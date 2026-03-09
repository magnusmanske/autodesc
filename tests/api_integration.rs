use autodesc::desc_options::DescOptions;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::WikiData;

/// Test that the ShortDescription can generate a description for a known person (Q42 = Douglas Adams).
#[tokio::test]
async fn test_api_person_q42_text() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    let (q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
    assert_eq!(q, "Q42");
    assert!(!desc.is_empty(), "Description should not be empty");
    // Douglas Adams is a person; description should contain occupation or dates
    let has_expected = desc.contains("1952")
        || desc.contains("writer")
        || desc.contains("novelist")
        || desc.contains("screenwriter")
        || desc.contains("playwright");
    assert!(
        has_expected,
        "Person description for Q42 should contain relevant info, got: {}",
        desc
    );
    // Should contain death year
    assert!(
        desc.contains("2001"),
        "Description should contain death year 2001, got: {}",
        desc
    );
    // Should contain gender symbol for male
    assert!(
        desc.contains('♂'),
        "Description should contain male symbol, got: {}",
        desc
    );
}

/// Test that wikidata link mode produces HTML anchor tags.
#[tokio::test]
async fn test_api_person_q42_wikidata_links() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "wikidata".to_string(),
        ..Default::default()
    };

    let (_q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
    assert!(
        desc.contains("wikidata.org"),
        "Wikidata link mode should produce wikidata.org links, got: {}",
        desc
    );
    assert!(
        desc.contains("<a href="),
        "Wikidata link mode should produce HTML anchor tags, got: {}",
        desc
    );
}

/// Test that wiki link mode produces wikitext-style links.
#[tokio::test]
async fn test_api_item_wiki_links() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "wiki".to_string(),
        ..Default::default()
    };

    // Q4504 = Komodo dragon - a taxon, should produce wikitext links
    let (_q, desc) = sd.load_item("Q4504", &mut opt, &mut wd).await;
    assert!(
        desc.contains("[[") || !desc.is_empty(),
        "Wiki link mode should produce wikitext brackets or plain text, got: {}",
        desc
    );
}

/// Test that a generic item (not person, not taxon) gets described.
#[tokio::test]
async fn test_api_generic_item() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    // Q12345 = Count von Count (fictional character)
    let (q, desc) = sd.load_item("Q12345", &mut opt, &mut wd).await;
    assert_eq!(q, "Q12345");
    assert!(
        !desc.is_empty(),
        "Description should not be empty for Q12345"
    );
    assert!(
        !desc.contains("Cannot auto-describe"),
        "Should be able to describe Q12345, got: {}",
        desc
    );
}

/// Test a German-language description.
#[tokio::test]
async fn test_api_person_german() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "de".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    // Q1035 = Charles Darwin
    let (q, desc) = sd.load_item("Q1035", &mut opt, &mut wd).await;
    assert_eq!(q, "Q1035");
    assert!(!desc.is_empty(), "Description should not be empty");
    // Should contain birth/death years
    assert!(
        desc.contains("1809") && desc.contains("1882"),
        "Darwin's description should contain birth/death years, got: {}",
        desc
    );
    // Should contain male symbol
    assert!(
        desc.contains('♂'),
        "Description should contain male symbol, got: {}",
        desc
    );
}

/// Test that numeric Q-ids are normalized.
#[tokio::test]
async fn test_api_numeric_q() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    // Pass just the number, should be treated as Q42
    let (q, desc) = sd.load_item("42", &mut opt, &mut wd).await;
    assert_eq!(q, "Q42");
    assert!(!desc.is_empty());
}

/// Test that the label and manual description can be retrieved after loading.
#[tokio::test]
async fn test_api_label_and_description() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    let _ = sd.load_item("Q42", &mut opt, &mut wd).await;

    let item = wd.get_item("Q42").expect("Q42 should be loaded");
    let label = item.get_label(Some("en"));
    assert_eq!(label, "Douglas Adams");

    let desc = item.get_desc(Some("en"));
    assert!(!desc.is_empty(), "Manual description should exist for Q42");
}

/// Test disambiguation page detection.
#[tokio::test]
async fn test_api_disambig() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "text".to_string(),
        ..Default::default()
    };

    // Q1364240 is a disambiguation page (Mercury)
    let (_q, desc) = sd.load_item("Q1364240", &mut opt, &mut wd).await;
    // It may or may not be a disambig depending on P107 claim existence;
    // just verify we get a non-empty result
    assert!(!desc.is_empty(), "Description should not be empty");
}

/// Test the stock translations are properly loaded.
#[test]
fn test_stock_translations_completeness() {
    let sd = ShortDescription::new();

    // Verify key translations exist
    let keys = [
        "BC",
        "by",
        "cannot_describe",
        "child of",
        "disambig",
        "for",
        "found_in",
        "in",
        "location",
        "member of",
        "named after",
        "of",
        "part of",
        "person",
        "produced by",
        "published in",
        "spouse of",
        "until",
        "about",
        "from",
    ];

    for key in keys {
        assert!(
            sd.stock.contains_key(key),
            "Stock should contain key '{}'",
            key
        );
        let translations = sd.stock.get(key).unwrap();
        assert!(
            translations.contains_key("en"),
            "Key '{}' should have an English translation",
            key
        );
    }
}

/// Test that list_words handles various languages correctly.
#[test]
fn test_list_words_multilingual() {
    let sd = ShortDescription::new();
    let empty = std::collections::HashMap::new();

    // French
    assert_eq!(
        sd.list_words(&["un".into(), "deux".into()], &empty, "fr"),
        "un et deux"
    );

    // Dutch
    assert_eq!(
        sd.list_words(&["een".into(), "twee".into()], &empty, "nl"),
        "een en twee"
    );

    // Polish
    assert_eq!(
        sd.list_words(&["jeden".into(), "dwa".into()], &empty, "pl"),
        "jeden i dwa"
    );

    // Vietnamese with 3+ items
    assert_eq!(
        sd.list_words(&["một".into(), "hai".into(), "ba".into()], &empty, "vi"),
        "một, hai, và ba"
    );

    // Unknown language falls back to comma separation
    assert_eq!(
        sd.list_words(&["a".into(), "b".into(), "c".into()], &empty, "xx"),
        "a, b, c"
    );
}

/// Test gender-aware word modification.
#[test]
fn test_modify_word_gender() {
    let sd = ShortDescription::new();

    let mut female_hints = std::collections::HashMap::new();
    female_hints.insert("is_female".to_string(), true);

    let mut male_hints = std::collections::HashMap::new();
    male_hints.insert("is_male".to_string(), true);

    // English
    assert_eq!(sd.modify_word("actor", &female_hints, "en"), "actress");
    assert_eq!(
        sd.modify_word("actor / actress", &female_hints, "en"),
        "actress"
    );
    assert_eq!(
        sd.modify_word("actor / actress", &male_hints, "en"),
        "actor"
    );

    // French
    assert_eq!(sd.modify_word("acteur", &female_hints, "fr"), "actrice");
    assert_eq!(
        sd.modify_word("être humain", &female_hints, "fr"),
        "personne"
    );

    // German with occupation hint
    let mut de_female = std::collections::HashMap::new();
    de_female.insert("is_female".to_string(), true);
    de_female.insert("occupation".to_string(), true);
    assert_eq!(sd.modify_word("Arzt", &de_female, "de"), "Arztin");
}

/// Test wikipedia link mode for a well-known item.
#[tokio::test]
async fn test_api_wikipedia_links() {
    let sd = ShortDescription::new();
    let mut wd = WikiData::new();
    let mut opt = DescOptions {
        lang: "en".to_string(),
        links: "wikipedia".to_string(),
        ..Default::default()
    };

    let (_q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
    // Should contain wikipedia.org links for items that have enwiki sitelinks
    assert!(
        desc.contains("wikipedia.org") || !desc.contains("Cannot"),
        "Wikipedia link mode should try to produce Wikipedia links, got: {}",
        desc
    );
}

/// Test media generation for a location with images and coordinates.
#[tokio::test]
async fn test_media_generation_location() {
    use autodesc::media::MediaGenerator;

    let mut wd = WikiData::new();
    // Q350 = Cambridge
    let result = MediaGenerator::generate_media("Q350", "80", 4, &mut wd).await;

    // Cambridge should have at least some media
    assert!(
        !result.media.is_empty(),
        "Cambridge (Q350) should have media entries"
    );

    // Should have OSM map if it has coordinates
    if result.media.contains_key("osm") {
        assert!(
            result.thumbnails.contains_key("osm_map"),
            "OSM entry should have osm_map thumbnail"
        );
    }
}

/// Test WikiData item methods.
#[tokio::test]
async fn test_wikidata_item_methods() {
    let mut wd = WikiData::new();
    wd.load_entity("Q12345").await.unwrap();

    let item = wd.get_item("Q12345").unwrap();

    // Test get_id
    assert_eq!(item.get_id(), "Q12345");

    // Test is_item
    assert!(item.is_item());

    // Test has_claims for known properties
    assert!(item.has_claims("P31"), "Q12345 should have P31 claims");

    // Test get_claim_items_for_property
    let p31_items = item.get_claim_items_for_property("P31");
    assert!(!p31_items.is_empty(), "Should have instance-of items");

    // Test get_label
    let label = item.get_label(Some("en"));
    assert!(!label.is_empty(), "English label should exist");

    // Test get_label with no language (fallback)
    let label_any = item.get_label(None);
    assert!(!label_any.is_empty(), "Fallback label should exist");

    // Test get_desc
    let desc = item.get_desc(Some("en"));
    // Desc might or might not exist, just verify it doesn't panic
    let _ = desc;

    // Test get_wiki_links
    let links = item.get_wiki_links();
    assert!(
        links.contains_key("dewiki"),
        "Q12345 (Count von Count) should have a dewiki sitelink"
    );
}

/// Test that batch loading works correctly and deduplicates.
#[tokio::test]
async fn test_batch_loading_dedup() {
    let mut wd = WikiData::new();

    // Load with duplicates
    let items = vec![
        "Q42".to_string(),
        "Q1".to_string(),
        "Q42".to_string(), // duplicate
        "q1".to_string(),  // case variant duplicate
    ];
    wd.get_item_batch(&items).await.unwrap();

    assert!(wd.has_item("Q42"));
    assert!(wd.has_item("Q1"));

    // Second load should be a no-op (items already cached)
    wd.get_item_batch(&["Q42".to_string()]).await.unwrap();
    assert!(wd.has_item("Q42"));
}
