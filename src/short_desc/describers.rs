use std::sync::OnceLock;

use regex::Regex;

use crate::desc_options::DescOptions;
use crate::wikidata::{WikiData, WikiDataItem};

use super::word_helpers::{clean_spaces, split_link, uc_first, Add2DescArgs, WordHints};
use super::ShortDescription;

fn entity_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^.+?entity/").expect("regex is valid"))
}

/// Maps a taxon-rank Q-id string to its index in the taxa_cache array.
fn taxon_rank_index(q: &str) -> Option<usize> {
    match q {
        "Q767728" => Some(0), // variety
        "Q68947" => Some(1),  // subspecies
        "Q7432" => Some(2),   // species
        "Q34740" => Some(3),  // genus
        "Q35409" => Some(4),  // family
        "Q36602" => Some(5),  // order
        "Q37517" => Some(6),  // class
        "Q38348" => Some(7),  // phylum
        "Q36732" => Some(8),  // kingdom
        _ => None,
    }
}

impl ShortDescription {
    /// Generate a description for a person.
    pub(super) async fn describe_person(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let mut load_items: Vec<(u64, String)> = Vec::new();
        Self::add_items_from_claims(claims, 106, &mut load_items); // Occupation
        Self::add_items_from_claims(claims, 39, &mut load_items); // Office
        Self::add_items_from_claims(claims, 27, &mut load_items); // Country of citizenship
        Self::add_items_from_claims(claims, 166, &mut load_items); // Award received
        Self::add_items_from_claims(claims, 31, &mut load_items); // Instance of
        Self::add_items_from_claims(claims, 22, &mut load_items); // Father
        Self::add_items_from_claims(claims, 25, &mut load_items); // Mother
        Self::add_items_from_claims(claims, 26, &mut load_items); // Spouse
        Self::add_items_from_claims(claims, 463, &mut load_items); // Member of
        Self::add_items_from_claims(claims, 800, &mut load_items); // Notable work

        let is_male = Self::has_pq(claims, 21, 6581097);
        let is_female = Self::has_pq(claims, 21, 6581072);

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let lang = &opt.lang;
        let mut h: Vec<String> = Vec::new();

        // Nationality
        let nationality_items = item_labels.get(&27).cloned().unwrap_or_default();
        let mut h2 = String::new();

        for (k, v) in nationality_items.iter().enumerate() {
            let (_full, before, inner, after) = split_link(v);
            let s = self.get_nationality_from_country(&inner, claims, lang);
            if k == 0 {
                h2 = format!("{}{}{}", before, s, after);
            } else {
                h2 = format!("{}-{}{}{}", h2, before, s.to_lowercase(), after);
            }
        }
        if !h2.is_empty() {
            h.push(h2);
        }

        // Occupation
        let ol = h.len();
        let hints = WordHints {
            is_male,
            is_female,
            occupation: true,
        };
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[31, 106],
                hints: &hints,
                prefix: None,
                txt_key: None,
            },
            lang,
        );
        if h.len() == ol {
            h.push(self.txt("person", lang));
        }

        // Office
        let office_hints = WordHints {
            is_male,
            is_female,
            ..Default::default()
        };
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[39],
                hints: &office_hints,
                prefix: Some(","),
                txt_key: None,
            },
            lang,
        );

        // Dates
        let born = WikiData::get_year(claims, 569, lang, &self.stock);
        let died = WikiData::get_year(claims, 570, lang, &self.stock);
        if !born.is_empty() && !died.is_empty() {
            h.push(format!("({}–{})", born, died));
        } else if !born.is_empty() {
            h.push(format!("(*{})", born));
        } else if !died.is_empty() {
            h.push(format!("(†{})", died));
        }

        // Gender symbols
        if is_female {
            h.push("♀".to_string());
        }
        if is_male {
            h.push("♂".to_string());
        }

        let empty_hints = WordHints::default();

        // Awards
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[166],
                hints: &empty_hints,
                prefix: Some(";"),
                txt_key: None,
            },
            lang,
        );

        // Member of
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[463],
                hints: &empty_hints,
                prefix: Some(";"),
                txt_key: Some("member of"),
            },
            lang,
        );

        // Child of (father/mother)
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[22, 25],
                hints: &empty_hints,
                prefix: Some(";"),
                txt_key: Some("child of"),
            },
            lang,
        );

        // Spouse
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[26],
                hints: &empty_hints,
                prefix: Some(";"),
                txt_key: Some("spouse of"),
            },
            lang,
        );

        // Notable work
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[800],
                hints: &empty_hints,
                prefix: Some(";"),
                txt_key: Some("notable work"),
            },
            lang,
        );

        if h.is_empty() {
            h.push(self.txt("person", lang));
        }

        let result = uc_first(&h.join(" "));
        (q.to_string(), clean_spaces(&result))
    }

    /// Generate a description for a taxon using SPARQL.
    pub(super) async fn describe_taxon(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let sparql = format!(
            "SELECT ?taxon ?taxonRank ?taxonRankLabel ?parentTaxon ?taxonLabel ?taxonName {{ wd:{q} \
             wdt:P171* ?taxon . ?taxon wdt:P171 ?parentTaxon . ?taxon wdt:P225 ?taxonName . ?taxon wdt:P105 ?taxonRank . \
             SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"[AUTO_LANGUAGE],{lang}\". }} }}",
            q = q,
            lang = opt.lang
        );

        let url = format!(
            "https://query.wikidata.org/bigdata/namespace/wdq/sparql?format=json&query={}",
            urlencoding::encode(&sparql)
        );

        let body = match wd.get_json(&url).await {
            Ok(v) => v,
            Err(_) => return self.describe_generic(q, claims, opt, wd).await,
        };

        let bindings = body
            .get("results")
            .and_then(|r| r.get("bindings"))
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();

        let entity_re = entity_url_re();

        let mut taxon_name: Option<String> = None;
        let mut taxa_cache: Vec<Option<serde_json::Value>> = vec![None; 9];
        let mut load_items: Vec<(u64, String)> = Vec::new();

        for binding in &bindings {
            let taxon_q = entity_re
                .replace_all(
                    binding
                        .get("taxon")
                        .and_then(|t| t.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    "",
                )
                .to_string();
            let taxon_rank = entity_re
                .replace_all(
                    binding
                        .get("taxonRank")
                        .and_then(|t| t.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    "",
                )
                .to_string();

            if taxon_q == q {
                load_items.push((0, taxon_rank.clone()));
                if let Some(tn) = binding
                    .get("taxonName")
                    .and_then(|t| t.get("value"))
                    .and_then(|v| v.as_str())
                {
                    taxon_name = Some(tn.to_string());
                }
            }

            if let Some(rank_id) = taxon_rank_index(&taxon_rank) {
                if rank_id < taxa_cache.len() {
                    taxa_cache[rank_id] = Some(binding.clone());
                }
            }
        }

        for binding in taxa_cache.iter().flatten() {
            let taxon_label = binding
                .get("taxonLabel")
                .and_then(|t| t.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let taxon_name_val = binding
                .get("taxonName")
                .and_then(|t| t.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if taxon_label.to_lowercase() != taxon_name_val.to_lowercase() {
                let taxon_q = entity_re
                    .replace_all(
                        binding
                            .get("taxon")
                            .and_then(|t| t.get("value"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        "",
                    )
                    .to_string();
                load_items.push((0, taxon_q));
                break;
            }
        }

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let labels_0 = item_labels.get(&0).cloned().unwrap_or_default();

        if labels_0.is_empty() {
            return self.describe_generic(q, claims, opt, wd).await;
        }

        let mut h_parts: Vec<String> = vec![labels_0[0].clone()];
        if labels_0.len() >= 2 {
            h_parts[0] = format!(
                "{} {} {}",
                h_parts[0],
                self.txt("of", &opt.lang),
                labels_0[1]
            );
        }

        if let Some(name) = &taxon_name {
            h_parts.push(format!("[{}]", name));
        }

        let result = uc_first(&h_parts.join(", "));
        (q.to_string(), clean_spaces(&result))
    }

    /// Generate a generic description for non-person, non-taxon items.
    pub(super) async fn describe_generic(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let mut load_items: Vec<(u64, String)> = Vec::new();

        Self::add_items_from_claims(claims, 361, &mut load_items); // Part of
        Self::add_items_from_claims(claims, 279, &mut load_items); // Subclass of
        Self::add_items_from_claims(claims, 1269, &mut load_items); // Facet of
        Self::add_items_from_claims(claims, 31, &mut load_items); // Instance of
        Self::add_items_from_claims(claims, 60, &mut load_items); // Astronomical object

        Self::add_items_from_claims(claims, 175, &mut load_items); // Performer
        Self::add_items_from_claims(claims, 86, &mut load_items); // Composer
        Self::add_items_from_claims(claims, 170, &mut load_items); // Creator
        Self::add_items_from_claims(claims, 57, &mut load_items); // Director
        Self::add_items_from_claims(claims, 162, &mut load_items); // Producer
        Self::add_items_from_claims(claims, 50, &mut load_items); // Author
        Self::add_items_from_claims(claims, 61, &mut load_items); // Discoverer/inventor

        Self::add_items_from_claims(claims, 17, &mut load_items); // Country
        Self::add_items_from_claims(claims, 131, &mut load_items); // Admin unit

        Self::add_items_from_claims(claims, 495, &mut load_items); // Country of origin
        Self::add_items_from_claims(claims, 159, &mut load_items); // Headquarters

        Self::add_items_from_claims(claims, 306, &mut load_items); // OS
        Self::add_items_from_claims(claims, 400, &mut load_items); // Platform
        Self::add_items_from_claims(claims, 176, &mut load_items); // Manufacturer

        Self::add_items_from_claims(claims, 123, &mut load_items); // Publisher
        Self::add_items_from_claims(claims, 264, &mut load_items); // Record label

        Self::add_items_from_claims(claims, 105, &mut load_items); // Taxon rank
        Self::add_items_from_claims(claims, 138, &mut load_items); // Named after
        Self::add_items_from_claims(claims, 171, &mut load_items); // Parent taxon

        Self::add_items_from_claims(claims, 1433, &mut load_items); // Published in
        Self::add_items_from_claims(claims, 571, &mut load_items); // Inception
        Self::add_items_from_claims(claims, 576, &mut load_items); // Until
        Self::add_items_from_claims(claims, 585, &mut load_items); // Point in time
        Self::add_items_from_claims(claims, 703, &mut load_items); // Found in taxon
        Self::add_items_from_claims(claims, 1080, &mut load_items); // From fictional universe
        Self::add_items_from_claims(claims, 1441, &mut load_items); // Present in work
        Self::add_items_from_claims(claims, 921, &mut load_items); // Main topic

        Self::add_items_from_claims(claims, 425, &mut load_items); // Field of profession
        Self::add_items_from_claims(claims, 59, &mut load_items); // Constellation

        Self::add_items_from_claims(claims, 1082, &mut load_items); // Population

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let lang = &opt.lang;
        let empty_hints = WordHints::default();
        let mut h: Vec<String> = Vec::new();

        // Publication date
        let pubdate = WikiData::get_year(claims, 577, lang, &self.stock);
        if !pubdate.is_empty() {
            h.push(pubdate);
        }

        // Instance/subclass/etc
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[279, 31, 1269, 60, 105],
                hints: &empty_hints,
                prefix: None,
                txt_key: None,
            },
            lang,
        );

        // Location
        let sep = " / ";
        let h2: Vec<String> = item_labels.get(&131).cloned().unwrap_or_default();
        let h3: Vec<String> = item_labels.get(&17).cloned().unwrap_or_default();

        if h.is_empty() && (!h2.is_empty() || !h3.is_empty()) {
            h.push(self.txt("location", lang));
        }

        if !h2.is_empty() && !h3.is_empty() {
            h.push(format!(
                "{} {}, {}",
                self.txt("in", lang),
                h2.join(sep),
                h3.join(sep)
            ));
        } else if !h2.is_empty() {
            h.push(format!("{} {}", self.txt("in", lang), h2.join(sep)));
        } else if !h3.is_empty() {
            h.push(format!("{} {}", self.txt("in", lang), h3.join(sep)));
        }

        // Population
        let pop_claims = claims
            .get("P1082")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(best) = WikiDataItem::get_best_quantity(&pop_claims) {
            let pop_label = wd
                .get_item("P1082")
                .map(|i| i.get_label(Some(lang)))
                .unwrap_or_else(|| "population".to_string());
            h.push(format!(", {} {}", pop_label, best));
        }

        // Creator etc
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[175, 86, 170, 57, 50, 61, 176],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("by"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[162],
                hints: &empty_hints,
                prefix: Some(","),
                txt_key: Some("produced by"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[306, 400],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("for"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[264, 123],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("from"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[361],
                hints: &empty_hints,
                prefix: Some(","),
                txt_key: Some("part of"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[138],
                hints: &empty_hints,
                prefix: Some(","),
                txt_key: Some("named after"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[425],
                hints: &empty_hints,
                prefix: Some(","),
                txt_key: Some("in the field of"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[171],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("of"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[59],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("in the constellation"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[1433],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("published in"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[585],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("in"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[703],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("found_in"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[1080, 1441],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("from"),
            },
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            Add2DescArgs {
                props: &[921],
                hints: &empty_hints,
                prefix: None,
                txt_key: Some("about"),
            },
            lang,
        );

        // Inception / Until dates
        let inception_year = WikiData::get_year(claims, 571, lang, &self.stock);
        if !inception_year.is_empty() {
            h.push(format!(", {} {}", self.txt("from", lang), inception_year));
        }
        let until_year = WikiData::get_year(claims, 576, lang, &self.stock);
        if !until_year.is_empty() {
            h.push(format!(", {} {}", self.txt("until", lang), until_year));
        }

        // Origin (headquarters, country of origin)
        let h2: Vec<String> = item_labels.get(&159).cloned().unwrap_or_default();
        let h3: Vec<String> = item_labels.get(&495).cloned().unwrap_or_default();
        if !h2.is_empty() && !h3.is_empty() {
            h.push(format!(
                "{} {}, {}",
                self.txt("from", lang),
                h2.join(sep),
                h3.join(sep)
            ));
        } else if !h2.is_empty() {
            h.push(format!("{} {}", self.txt("from", lang), h2.join(sep)));
        } else if !h3.is_empty() {
            h.push(format!("{} {}", self.txt("from", lang), h3.join(sep)));
        }

        // Fallback
        if h.is_empty() {
            let fallback = format!("<i>{}</i>", self.txt("cannot_describe", lang));
            return (q.to_string(), fallback);
        }

        let result = uc_first(&h.join(" "));
        (q.to_string(), clean_spaces(&result))
    }
}
