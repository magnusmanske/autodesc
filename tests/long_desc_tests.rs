use autodesc::desc_options::DescOptions;
use autodesc::long_desc::LongDescGenerator;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::WikiData;
use autodesc::wikidata_item::WikiDataItem;
use serde_json::{json, Value};
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
    wd.items
        .insert(id.to_string(), WikiDataItem::new(fake_item(id, label_en)));
}

fn insert_item_json(wd: &mut WikiData, id: &str, raw: Value) {
    wd.items.insert(id.to_string(), WikiDataItem::new(raw));
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q100", &claims, &opt, &mut wd).await;
    assert!(result.is_none(), "Non-person should return None");
}

/// Unsupported language returns None.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_returns_none_for_unsupported_lang() {
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
        lang: "ja".to_string(), // Japanese is not in LONG_DESC_LANGUAGES
        links: "text".to_string(),
        ..Default::default()
    };

    let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
    assert!(result.is_none(), "Unsupported language should return None");
}

/// Dutch long description uses Dutch pronouns and month names.
#[tokio::test(flavor = "multi_thread")]
async fn test_long_desc_nl() {
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
        lang: "nl".to_string(),
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
        lang: "fr".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
        lang: "en".to_string(),
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
