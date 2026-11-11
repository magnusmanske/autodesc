use autodesc::ItemId;
use autodesc::Lang;
use autodesc::desc_options::DescOptions;
use autodesc::long_desc::LongDescGenerator;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::WikiData;
use autodesc::wikidata_item::WikiDataItem;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Fake data helpers ────────────────────────────────────────────────────────

fn fake_wbgetentities(entities: Value) -> Value {
    json!({ "entities": entities })
}

fn fake_item(id: &str, label_en: &str) -> Value {
    json!({
        "type": "item",
        "id": id,
        "ns": 0,
        "labels": { "en": { "language": "en", "value": label_en } },
        "descriptions": {},
        "claims": {},
        "sitelinks": {}
    })
}

fn fake_item_with_sitelink(id: &str, label_en: &str, enwiki_title: &str) -> Value {
    json!({
        "type": "item",
        "id": id,
        "ns": 0,
        "labels": { "en": { "language": "en", "value": label_en } },
        "descriptions": {},
        "claims": {},
        "sitelinks": {
            "enwiki": { "site": "enwiki", "title": enwiki_title }
        }
    })
}

// Fake related-items response used by most tests.
fn fake_related_entities() -> Value {
    json!({
        "Q145":   fake_item_with_sitelink("Q145", "United Kingdom", "United Kingdom"),
        "Q36180": fake_item_with_sitelink("Q36180", "writer", "Writer"),
        "Q84":    fake_item_with_sitelink("Q84", "London", "London"),
        "Q350":   fake_item("Q350", "Cambridge"),
        "Q5":     fake_item("Q5", "human"),
        "Q6581097": fake_item("Q6581097", "male"),
        "Q6581072": fake_item("Q6581072", "female"),
        "Q8":     fake_item("Q8", "happiness"),
        "Q937":   fake_item("Q937", "Albert Einstein"),
        "Q131":   fake_item("Q131", "Marie Curie"),
        "Q3918":  fake_item("Q3918", "university"),
        "Q1": {
            "type": "item",
            "id": "Q1",
            "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "United Kingdom" },
                "nl": { "language": "nl", "value": "Verenigd Koninkrijk" },
                "fr": { "language": "fr", "value": "Royaume-Uni" }
            },
            "descriptions": {},
            "claims": {},
            "sitelinks": {}
        }
    })
}

/// Claims for a deceased male British writer (born 11 March 1952, died 11 May 2001).
fn claims_male_writer_deceased() -> Value {
    json!({
        // P31 = instance of human (Q5)
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }],
        // P21 = sex or gender: male (Q6581097)
        "P21": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q6581097" } } } }],
        // P27 = country of citizenship: United Kingdom (Q145)
        "P27": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q145" } } } }],
        // P106 = occupation: writer (Q36180)
        "P106": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q36180" } } } }],
        // P569 = date of birth
        "P569": [{ "mainsnak": { "datavalue": { "value": { "time": "+1952-03-11T00:00:00Z", "precision": 11 } } } }],
        // P19 = place of birth: London (Q84)
        "P19": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q84" } } } }],
        // P570 = date of death
        "P570": [{ "mainsnak": { "datavalue": { "value": { "time": "+2001-05-11T00:00:00Z", "precision": 11 } } } }],
        // P20 = place of death: London (Q84)
        "P20": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q84" } } } }]
    })
}

/// Claims for a living female scientist.
fn claims_female_scientist_alive() -> Value {
    json!({
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }],
        "P21": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q6581072" } } } }],
        "P569": [{ "mainsnak": { "datavalue": { "value": { "time": "+1980-06-15T00:00:00Z", "precision": 11 } } } }]
    })
}

/// Claims for a non-person item (a book).
fn claims_non_person() -> Value {
    json!({
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q571" } } } }]
    })
}

/// Pre-insert a fake item for the given Q-id into `wd`.
fn insert_item(wd: &mut WikiData, id: &str, label_en: &str) {
    wd.items.insert(
        ItemId::parse_lenient(id).unwrap(),
        WikiDataItem::new(fake_item(id, label_en)),
    );
}

fn insert_item_json(wd: &mut WikiData, id: &str, raw: Value) {
    wd.items
        .insert(ItemId::parse_lenient(id).unwrap(), WikiDataItem::new(raw));
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// English long description for a deceased male writer — smoke test.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_en_male_writer_deceased() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_some(), "Should produce a long description");
    let desc = result.unwrap();

    assert!(desc.contains("Douglas Adams"), "Name in output: {desc}");
    assert!(desc.contains("was"), "Past tense for deceased: {desc}");
    assert!(desc.contains("1952"), "Birth year: {desc}");
    assert!(desc.contains("2001"), "Death year: {desc}");
    assert!(desc.contains("writer"), "Occupation: {desc}");
    assert!(desc.contains("London"), "Birth/death place: {desc}");
    // Should not contain raw HTML in text mode
    assert!(
        !desc.contains("<a href"),
        "No HTML links in text mode: {desc}"
    );
}

/// For a living female person, "is" (not "was") and female pronouns are used.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_en_female_alive() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q999", "Jane Doe");

    let sd = ShortDescription::new();
    let claims = claims_female_scientist_alive();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q999", &claims, &opt, &mut wd).await;
    assert!(result.is_some());
    let desc = result.unwrap();

    // Alive: no death year in output
    assert!(
        !desc.contains("died"),
        "No death info for living person: {desc}"
    );
    // Birth section uses "She was born"
    assert!(
        desc.contains("She was born"),
        "Female pronoun in birth: {desc}"
    );
    assert!(desc.contains("1980"), "Birth year: {desc}");
}

/// Non-person items return None.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_returns_none_for_non_person() {
    let mock_server = MockServer::start().await;
    // The mock should never be called since we bail out early
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(json!({}))))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q100", "Some Book");

    let sd = ShortDescription::new();
    let claims = claims_non_person();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q100", &claims, &opt, &mut wd).await;
    assert!(result.is_none(), "Non-person should return None");
}

/// Unsupported language returns None.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_returns_none_for_unsupported_lang() {
    autodesc::lang_type::init_langs().await;
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(json!({}))))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Someone");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::parse("ja").unwrap(), // Japanese is not in LONG_DESC_LANGUAGES
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_none(), "Unsupported language should return None");
}

/// Dutch long description uses Dutch pronouns and month names.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_nl() {
    autodesc::lang_type::init_langs().await;
    let dutch_entities = json!({
        "Q145": {
            "type": "item", "id": "Q145", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "United Kingdom" },
                "nl": { "language": "nl", "value": "Verenigd Koninkrijk" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        },
        "Q36180": {
            "type": "item", "id": "Q36180", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "writer" },
                "nl": { "language": "nl", "value": "schrijver" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        },
        "Q84": {
            "type": "item", "id": "Q84", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "London" },
                "nl": { "language": "nl", "value": "Londen" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        }
    });

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(dutch_entities)))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item_json(
        &mut wd,
        "Q42",
        json!({
            "type": "item", "id": "Q42", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "Douglas Adams" },
                "nl": { "language": "nl", "value": "Douglas Adams" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        }),
    );

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::parse("nl").unwrap(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(
        result.is_some(),
        "Dutch long description should be produced"
    );
    let desc = result.unwrap();

    assert!(desc.contains("Douglas Adams"), "Name in output: {desc}");
    // Dutch uses "Hij werd geboren" for male
    assert!(
        desc.contains("Hij werd geboren"),
        "Dutch male birth sentence: {desc}"
    );
    assert!(desc.contains("1952"), "Birth year: {desc}");
    assert!(desc.contains("2001"), "Death year: {desc}");
}

/// French long description uses French pronouns.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_fr() {
    autodesc::lang_type::init_langs().await;
    let french_entities = json!({
        "Q145": {
            "type": "item", "id": "Q145", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "United Kingdom" },
                "fr": { "language": "fr", "value": "Royaume-Uni" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        },
        "Q36180": {
            "type": "item", "id": "Q36180", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "writer" },
                "fr": { "language": "fr", "value": "écrivain" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        },
        "Q84": {
            "type": "item", "id": "Q84", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "London" },
                "fr": { "language": "fr", "value": "Londres" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        }
    });

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(french_entities)))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item_json(
        &mut wd,
        "Q42",
        json!({
            "type": "item", "id": "Q42", "ns": 0,
            "labels": {
                "en": { "language": "en", "value": "Douglas Adams" },
                "fr": { "language": "fr", "value": "Douglas Adams" }
            },
            "descriptions": {}, "claims": {}, "sitelinks": {}
        }),
    );

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::parse("fr").unwrap(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(
        result.is_some(),
        "French long description should be produced"
    );
    let desc = result.unwrap();

    assert!(desc.contains("Douglas Adams"), "Name in output: {desc}");
    assert!(desc.contains("1952"), "Birth year: {desc}");
    assert!(desc.contains("2001"), "Death year: {desc}");
}

/// With `links=wikidata`, item references are wrapped in wikidata.org anchor tags.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_link_mode_wikidata() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "wikidata".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_some());
    let desc = result.unwrap();

    assert!(
        desc.contains("wikidata.org"),
        "Wikidata links present: {desc}"
    );
    assert!(desc.contains("<a href="), "HTML anchors present: {desc}");
}

/// With `links=wiki`, item references become wikitext-style `[[...]]` links.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_link_mode_wiki() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "wiki".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_some());
    let desc = result.unwrap();

    // "wiki" mode wraps the name in '''...''' and items with sitelinks in [[...]]
    assert!(
        desc.contains("'''Douglas Adams'''"),
        "Bold wiki title: {desc}"
    );
    assert!(
        desc.contains("[["),
        "Wikitext links for items with sitelinks: {desc}"
    );
}

/// With `links=wikipedia`, item references become Wikipedia HTML anchor tags.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_link_mode_wikipedia() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "wikipedia".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_some());
    let desc = result.unwrap();

    // Items with enwiki sitelinks should appear as Wikipedia links
    assert!(
        desc.contains("en.wikipedia.org"),
        "Wikipedia links present: {desc}"
    );
}

/// User-editable Wikidata text must be HTML-escaped in the generated result.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_escapes_html_in_labels() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "<script>alert(1)</script>");

    let sd = ShortDescription::new();
    let mut claims = claims_male_writer_deceased();
    claims["P513"] = json!([{
        "mainsnak": {
            "datavalue": { "type": "string", "value": "<img src=x onerror=alert(1)>" }
        }
    }]);
    let opt = DescOptions {
        lang: Lang::default(),
        links: "wikidata".to_string(),
        ..Default::default()
    };

    let desc = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd)
        .await
        .expect("Should produce a long description");

    assert!(
        desc.contains("<b>&lt;script&gt;alert(1)&lt;/script&gt;</b>"),
        "Title label must be escaped: {desc}"
    );
    assert!(
        desc.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "Birth name must be escaped: {desc}"
    );
    assert!(!desc.contains("<script>"), "No raw script tag: {desc}");
    assert!(!desc.contains("<img"), "No raw img tag: {desc}");
}

/// Wiki link mode must escape both labels and sitelink page titles.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_escapes_wiki_links() {
    let mut entities = fake_related_entities();
    entities["Q36180"]["labels"]["en"]["value"] = json!("<script>alert(1)</script>");
    entities["Q36180"]["sitelinks"]["enwiki"]["title"] = json!("<img src=x onerror=alert(1)>");

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(entities)))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let claims = claims_male_writer_deceased();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "wiki".to_string(),
        ..Default::default()
    };

    let desc = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd)
        .await
        .expect("Should produce a long description");

    assert!(
        desc.contains(
            "[[&lt;img src=x onerror=alert(1)&gt;|&lt;script&gt;alert(1)&lt;/script&gt;]]"
        ),
        "Wiki page title and label must be escaped: {desc}"
    );
    assert!(!desc.contains("<script>"), "No raw script tag: {desc}");
    assert!(!desc.contains("<img"), "No raw img tag: {desc}");
}

/// Multiple occupations are listed with "and" between the last two.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_multiple_occupations() {
    let entities = json!({
        "Q36180": fake_item("Q36180", "writer"),
        "Q245068": fake_item("Q245068", "comedian"),
        "Q84":    fake_item("Q84", "London"),
        "Q145":   fake_item("Q145", "United Kingdom")
    });

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(entities)))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q1", "Test Person");

    let claims = json!({
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }],
        "P21": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q6581097" } } } }],
        "P106": [
            { "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q36180" } } } },
            { "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q245068" } } } }
        ],
        "P570": [{ "mainsnak": { "datavalue": { "value": { "time": "+2000-01-01T00:00:00Z", "precision": 11 } } } }]
    });

    let sd = ShortDescription::new();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q1", &claims, &opt, &mut wd).await;
    assert!(result.is_some());
    let desc = result.unwrap();
    // Both occupations should appear
    assert!(desc.contains("writer"), "First occupation: {desc}");
    assert!(desc.contains("comedian"), "Second occupation: {desc}");
    // English list style: "writer and comedian"
    assert!(desc.contains("and"), "List conjunction: {desc}");
}

/// A person with no birth/death/occupation claims still gets a description.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_minimal_person() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(json!({}))))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q1", "Unknown Person");

    let claims = json!({
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }]
        // No gender, no dates, no occupation
    });

    let sd = ShortDescription::new();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q1", &claims, &opt, &mut wd).await;
    // Should still return Some (even if short), not panic
    assert!(
        result.is_some(),
        "Minimal person should still get a description"
    );
}

/// P107=Q215627 (person) also qualifies for long description (alternative P31 path).
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_p107_person() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(json!({}))))
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q1", "Some Person");

    let claims = json!({
        "P107": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q215627" } } } }]
    });

    let sd = ShortDescription::new();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q1", &claims, &opt, &mut wd).await;
    assert!(
        result.is_some(),
        "P107=Q215627 should produce a long description"
    );
}

/// Batch load is called once even when claims reference many items.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_batch_load_called() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .expect(1) // exactly one API call for the batch
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q42", "Douglas Adams");

    let sd = ShortDescription::new();
    let opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        ..Default::default()
    };

    LongDescGenerator::generate(&sd, "Q42", &claims_male_writer_deceased(), &opt, &mut wd).await;
    // MockServer verifies `.expect(1)` when dropped
}

/// `is_long_desc_available` reports correct languages.
#[test]
fn test_is_long_desc_available() {
    use autodesc::long_desc::is_long_desc_available;

    assert!(is_long_desc_available("en"));
    assert!(is_long_desc_available("nl"));
    assert!(is_long_desc_available("fr"));
    assert!(is_long_desc_available("de"));
    assert!(!is_long_desc_available("es"));
    assert!(!is_long_desc_available(""));
}

/// `load_item` with `mode=long` produces a long description for a person.
#[tokio::test(flavor = "multi_thread")]
async fn test_load_item_long_mode() {
    let mock_server = MockServer::start().await;

    // First call: load the main entity Q42
    let main_entity = json!({
        "Q42": {
            "type": "item", "id": "Q42", "ns": 0,
            "labels": { "en": { "language": "en", "value": "Douglas Adams" } },
            "descriptions": {},
            "claims": {
                "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }],
                "P21": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q6581097" } } } }],
                "P106": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q36180" } } } }],
                "P569": [{ "mainsnak": { "datavalue": { "value": { "time": "+1952-03-11T00:00:00Z", "precision": 11 } } } }],
                "P570": [{ "mainsnak": { "datavalue": { "value": { "time": "+2001-05-11T00:00:00Z", "precision": 11 } } } }]
            },
            "sitelinks": {}
        }
    });

    // Subsequent call(s): batch load of related items
    let related = json!({
        "Q36180": fake_item("Q36180", "writer"),
        "Q6581097": fake_item("Q6581097", "male")
    });

    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(main_entity.clone())),
        )
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(related)))
        .mount(&mock_server)
        .await;

    let sd = ShortDescription::new();
    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    let mut opt = DescOptions {
        lang: Lang::default(),
        links: "text".to_string(),
        mode: "long".to_string(),
        ..Default::default()
    };

    let (_q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
    assert!(
        !desc.is_empty(),
        "Long mode description should not be empty"
    );
    assert!(
        desc.contains("Douglas Adams"),
        "Name in long description: {desc}"
    );
    assert!(
        desc.contains("1952"),
        "Birth year in long description: {desc}"
    );
}

// ─── Date precision, circa, P793 and spouse end date regressions ──────────────

/// Generate an English long description for a synthetic person with the given
/// claims, preloading any labels the claims refer to.
async fn generate_en(claims: Value, extra_items: &[(&str, &str)]) -> String {
    generate_in_lang(claims, "en", extra_items).await
}

async fn generate_in_lang(claims: Value, lang: &str, extra_items: &[(&str, &str)]) -> String {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/w/api.php"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_related_entities())),
        )
        .mount(&mock_server)
        .await;

    let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
    insert_item(&mut wd, "Q100", "Test Person");
    for (id, label) in extra_items {
        insert_item(&mut wd, id, label);
    }

    let sd = ShortDescription::new();
    let lang = if lang == "en" {
        Lang::default()
    } else {
        autodesc::lang_type::init_langs().await;
        Lang::parse(lang).unwrap()
    };
    let opt = DescOptions {
        lang,
        links: "text".to_string(),
        ..Default::default()
    };

    LongDescGenerator::generate(&sd, "Q100", &claims, &opt, &mut wd)
        .await
        .expect("long description should be produced")
}

fn claims_person() -> Value {
    json!({
        "P31": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }],
        "P21": [{ "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q6581072" } } } }]
    })
}

/// A precision-8 date of birth is a decade, not a year.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_decade_precision() {
    let mut claims = claims_person();
    claims["P569"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1940-00-00T00:00:00Z", "precision": 8 } } }
    }]);

    let desc = generate_en(claims, &[]).await;
    assert!(desc.contains("1940s"), "Decade should be rendered: {desc}");
    assert!(
        !desc.contains("born in 1940."),
        "Decade must not be rendered as a plain year: {desc}"
    );
}

/// P1480 = circa on a date of birth is rendered as "around".
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_circa_qualifier() {
    let mut claims = claims_person();
    claims["P569"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1960-00-00T00:00:00Z", "precision": 9 } } },
        "qualifiers": {
            "P1480": [{ "datavalue": { "value": { "entity-type": "item", "id": "Q5727902" } } }]
        }
    }]);

    let desc = generate_en(claims, &[]).await;
    assert!(
        desc.contains("around 1960"),
        "Circa should be rendered: {desc}"
    );
    assert!(
        !desc.contains("born in 1960."),
        "Circa date must not read as exact: {desc}"
    );
}

/// Circa and decade precision combine into "around the 1940s".
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_circa_decade_combines() {
    let mut claims = claims_person();
    claims["P569"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1940-00-00T00:00:00Z", "precision": 8 } } },
        "qualifiers": {
            "P1480": [{ "datavalue": { "value": { "entity-type": "item", "id": "Q5727902" } } }]
        }
    }]);

    let desc = generate_en(claims, &[]).await;
    assert!(
        desc.contains("around the 1940s"),
        "Circa decade should combine naturally: {desc}"
    );
}

/// P793 (significant event) must not present the subject as its agent, and a
/// P585 point in time is included when available.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_significant_event_not_agent() {
    let mut claims = claims_person();
    claims["P793"] = json!([{
        "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q201676" } } },
        "qualifiers": {
            "P585": [{ "datavalue": { "value": { "time": "+1934-07-06T00:00:00Z", "precision": 11 } } }]
        }
    }]);

    let desc = generate_en(claims, &[("Q201676", "funeral")]).await;
    assert!(
        !desc.contains("played a role in funeral"),
        "P793 must not invent an agent role: {desc}"
    );
    assert!(
        desc.contains("funeral"),
        "Event label should appear: {desc}"
    );
    assert!(
        desc.contains("1934"),
        "Event point in time should appear: {desc}"
    );
}

/// A spouse end date must not be rendered as "until on <date>", while exact
/// day-precision dates keep their existing format.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_spouse_end_date_preposition() {
    let mut claims = claims_person();
    claims["P569"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1952-03-11T00:00:00Z", "precision": 11 } } }
    }]);
    claims["P26"] = json!([{
        "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q105" } } },
        "qualifiers": {
            "P580": [{ "datavalue": { "value": { "time": "+1991-11-25T00:00:00Z", "precision": 11 } } }],
            "P582": [{ "datavalue": { "value": { "time": "+2001-05-11T00:00:00Z", "precision": 11 } } }]
        }
    }]);

    let desc = generate_en(claims, &[("Q105", "Jane Belson")]).await;
    assert!(!desc.contains("until on"), "No 'until on': {desc}");
    assert!(
        desc.contains("November 25, 1991"),
        "Start date format: {desc}"
    );
    assert!(desc.contains("May 11, 2001"), "End date format: {desc}");
    assert!(
        desc.contains("March 11, 1952"),
        "Day-precision birth date unchanged: {desc}"
    );
}

/// French day-precision dates keep their existing format.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_fr_exact_dates_unchanged() {
    let mut claims = claims_person();
    claims["P569"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1867-11-07T00:00:00Z", "precision": 11 } } }
    }]);
    claims["P570"] = json!([{
        "mainsnak": { "datavalue": { "value": { "time": "+1934-07-04T00:00:00Z", "precision": 11 } } }
    }]);

    let desc = generate_in_lang(claims, "fr", &[]).await;
    assert!(
        desc.contains("7 novembre 1867"),
        "French birth date format: {desc}"
    );
    assert!(
        desc.contains("4 juillet 1934"),
        "French death date format: {desc}"
    );
}
