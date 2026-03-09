use serde_json::Value;

use crate::short_desc::ShortDescription;
use crate::wikidata::WikiData;

use super::*;

const CFG: LangConfig = LangConfig {
    month_labels: [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    pronoun_subject_male: "He",
    pronoun_possessive_male: "His",
    pronoun_subject_female: "She",
    pronoun_possessive_female: "Her",
    pronoun_subject_neutral: "They",
    pronoun_possessive_neutral: "Their",
    be_present_singular: "is",
    be_past_singular: "was",
    be_present_neutral: "are",
    be_past_neutral: "were",
};

/// Separator for English lists: "a, b, and c".
fn get_sep_after_en(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if pos == 0 && len == 2 {
        " and "
    } else if len == pos + 2 {
        ", and "
    } else {
        ", "
    }
}

pub(super) struct LangEn;

impl LangGenerator for LangEn {
    fn add_first_sentence(
        &self,
        state: &mut LongDescState,
        q: &str,
        claims: &Value,
        sd: &ShortDescription,
        wd: &WikiData,
    ) {
        let label = get_main_title_label(q, wd, &state.lang);
        let initial_len = state.fragments.len();

        let bold = if state.newline == "\n" {
            format!("{} ", label)
        } else if state.newline == "\n\n" {
            format!("'''{}''' ", label)
        } else {
            format!("<b>{}</b> ", label)
        };
        state.push_text(&bold);

        let be = if state.is_dead { "was" } else { "is" };
        state.push_text(&format!("{} a ", be));

        // Nationalities
        let nationalities = get_claim_item_ids(claims, "P27");
        for (k, country_q) in nationalities.iter().enumerate() {
            let country_label = wd
                .get_item(country_q)
                .map(|i| i.get_label(Some(&state.lang)))
                .unwrap_or_default();
            let nationality =
                sd.get_nationality_from_country(&country_label, Some(country_q), &state.lang, wd);
            if k > 0 {
                state.push_text("-");
            }
            let nat = if k > 0 { nationality.to_lowercase() } else { nationality };
            state.push_text(&nat);
            if k + 1 == nationalities.len() {
                state.push_text(" ");
            }
        }

        // Occupations (use P2521/P3321 gendered label when available)
        let occupations = get_claim_item_ids(claims, "P106");
        for (k, occ_q) in occupations.iter().enumerate() {
            let sep = get_sep_after_en(occupations.len(), k);
            if state.is_female || state.is_male {
                if let Some(gendered) = wd
                    .get_item(occ_q)
                    .and_then(|i| i.get_gendered_label(&state.lang, state.is_female))
                {
                    state.push_text(&gendered);
                    state.push_text(sep);
                    continue;
                }
            }
            state.push_item(occ_q, "", sep);
        }

        state.push_text(". ");

        if nationalities.is_empty() && occupations.is_empty() {
            state.fragments.truncate(initial_len);
        }

        // Significant events
        let sig_events = get_claim_item_ids(claims, "P793");
        if !sig_events.is_empty() {
            let subj = state.pronoun_subject(&CFG);
            state.push_text(&format!("{} played a role in ", subj));
            for (k, ev_q) in sig_events.iter().enumerate() {
                state.push_item(ev_q, "", get_sep_after_en(sig_events.len(), k));
            }
            state.push_text(".");
        }

        state.push_newline();
    }

    fn add_birth_text(&self, state: &mut LongDescState, claims: &Value) {
        let birthdate =
            claims.get("P569").and_then(|v| v.as_array()).and_then(|a| a.first());
        let birthplace = get_first_claim_item(claims, "P19");
        let birthname = get_first_claim_string(claims, "P513");

        if birthdate.is_none() && birthplace.is_none() && birthname.is_none() {
            return;
        }

        let subj = state.pronoun_subject(&CFG);
        let be_past = state.be_past(&CFG);
        state.push_text(&format!("{} {} born ", subj, be_past));

        if let Some(name) = &birthname {
            state.push_text(&format!("<i>{}</i> ", name));
        }

        if let Some(claim) = birthdate {
            if let Some(date) = extract_claim_date(claim) {
                state.push_text(&self.render_date(&date, false));
                state.push_text(" ");
            }
        }

        if let Some(ref place_q) = birthplace {
            state.push_item(place_q, "in ", " ");
        }

        // Parents
        let father = get_first_claim_item(claims, "P22");
        let mother = get_first_claim_item(claims, "P25");
        if father.is_some() || mother.is_some() {
            state.push_text("to ");
            if let Some(ref f) = father {
                state.push_item(f, "", " ");
            }
            if father.is_some() && mother.is_some() {
                state.push_text("and ");
            }
            if let Some(ref m) = mother {
                state.push_item(m, "", " ");
            }
        }

        state.push_text(". ");
        state.push_newline();
    }

    fn add_work_text(&self, state: &mut LongDescState, claims: &Value) {
        let subj = state.pronoun_subject(&CFG);
        let poss = state.pronoun_possessive(&CFG);
        let be_present = state.be_present(&CFG);
        let be_past = state.be_past(&CFG);
        let is_dead = state.is_dead;

        // Education (P69)
        let alma = get_dated_items(claims, "P69", &[]);
        if !alma.is_empty() {
            state.push_text(&format!("{} studied at ", subj));
            for (k, item) in alma.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_en(alma.len(), k));
            }
            state.push_text(". ");
        }

        // Field of work (P136, P101)
        let mut fields: Vec<String> = get_claim_item_ids(claims, "P136");
        fields.extend(get_claim_item_ids(claims, "P101"));
        if !fields.is_empty() {
            let verb = if is_dead { "included" } else { "includes" };
            state.push_text(&format!("{} field of work {} ", poss, verb));
            for (k, q) in fields.iter().enumerate() {
                state.push_item(q, "", get_sep_after_en(fields.len(), k));
            }
            state.push_text(". ");
        }

        // Position held (P39) with qualifier P642 ("of")
        let positions = get_dated_items(claims, "P39", &["P642"]);
        if !positions.is_empty() {
            let verb = if is_dead {
                format!("{} ", be_past)
            } else {
                format!("{}/{} ", be_present, be_past)
            };
            state.push_text(&format!("{} {}", subj, verb));
            for (k, item) in positions.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(of_items) = item.qualifier_items.get("P642") {
                    if let Some(of_q) = of_items.first() {
                        state.push_item(of_q, "for ", " ");
                    }
                }
                state.push_text(get_sep_after_en(positions.len(), k));
            }
            state.push_text(". ");
        }

        // Member of (P463)
        let members = get_dated_items(claims, "P463", &[]);
        if !members.is_empty() {
            let verb = if is_dead {
                format!("{} ", be_past)
            } else {
                format!("{}/{} ", be_present, be_past)
            };
            state.push_text(&format!("{} {}a member of ", subj, verb));
            for (k, item) in members.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_en(members.len(), k));
            }
            state.push_text(". ");
        }

        // Employers (P108) with qualifier P794 ("as")
        let employers = get_dated_items(claims, "P108", &["P794"]);
        if !employers.is_empty() {
            state.push_text(&format!("{} worked for ", subj));
            for (k, item) in employers.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(job_items) = item.qualifier_items.get("P794") {
                    if let Some(job_q) = job_items.first() {
                        state.push_item(job_q, "as ", " ");
                    }
                }
                let sep = get_sep_after_en(employers.len(), k);
                if k + 1 < employers.len() {
                    state.push_text(&format!("{}for ", sep));
                } else {
                    state.push_text(sep);
                }
            }
            state.push_text(". ");
        }

        // Notable works (P800)
        let notable_works = get_claim_item_ids(claims, "P800");
        if !notable_works.is_empty() {
            let verb = if is_dead { "included" } else { "include" };
            state.push_text(&format!("{} notable works {} ", poss, verb));
            for (k, q) in notable_works.iter().enumerate() {
                state.push_item(q, "", get_sep_after_en(notable_works.len(), k));
            }
            state.push_text(". ");
        }

        state.push_newline();
    }

    fn add_family_text(&self, state: &mut LongDescState, claims: &Value) {
        let subj = state.pronoun_subject(&CFG);
        let poss = state.pronoun_possessive(&CFG);

        // Spouses (P26)
        let spouses = get_dated_items(claims, "P26", &[]);
        if !spouses.is_empty() {
            state.push_text(&format!("{} married ", subj));
            for (k, item) in spouses.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                if let Some(ref from) = item.date_from {
                    state.push_text(&self.render_date(from, false));
                    state.push_text(" ");
                }
                if let Some(ref to) = item.date_to {
                    state.push_text("(married until ");
                    state.push_text(&self.render_date(to, false));
                    state.push_text(") ");
                }
                state.push_text(get_sep_after_en(spouses.len(), k));
            }
            state.push_text(". ");
        }

        // Children (P40)
        let children = get_claim_item_ids(claims, "P40");
        if !children.is_empty() {
            state.push_text(&format!("{} children include ", poss));
            for (k, q) in children.iter().enumerate() {
                state.push_item(q, "", get_sep_after_en(children.len(), k));
            }
            state.push_text(". ");
        }

        state.push_newline();
    }

    fn add_death_text(&self, state: &mut LongDescState, claims: &Value) {
        let deathdate =
            claims.get("P570").and_then(|v| v.as_array()).and_then(|a| a.first());
        let deathplace = get_first_claim_item(claims, "P20");
        let has_deathcause = has_claims(claims, "P509");
        let has_killer = has_claims(claims, "P157");

        if deathdate.is_some() || deathplace.is_some() || has_deathcause || has_killer {
            let subj = state.pronoun_subject(&CFG);
            state.push_text(&format!("{} died ", subj));

            if has_deathcause {
                let causes = get_claim_item_ids(claims, "P509");
                push_simple_list(state, &causes, "of ", " ", get_sep_after_en);
            }

            if has_killer {
                let killers = get_claim_item_ids(claims, "P157");
                push_simple_list(state, &killers, "by ", " ", get_sep_after_en);
            }

            if let Some(claim) = deathdate {
                if let Some(date) = extract_claim_date(claim) {
                    state.push_text(&self.render_date(&date, false));
                    state.push_text(" ");
                }
            }

            if let Some(ref place_q) = deathplace {
                state.push_item(place_q, "in ", " ");
            }

            state.push_text(". ");
        }

        // Burial place (P119)
        if let Some(burial_q) = get_first_claim_item(claims, "P119") {
            let subj = state.pronoun_subject(&CFG);
            let be_past = state.be_past(&CFG);
            state.push_item(&burial_q, &format!("{} {} buried at ", subj, be_past), ". ");
        }
    }
}

impl LangEn {
    /// Render a Wikidata date as an English string.
    /// `no_prefix`: when true, omit the "in"/"on" preposition.
    fn render_date(&self, date: &WdDate, no_prefix: bool) -> String {
        let (year, year_str, month, day) = match parse_time(&date.time) {
            Some(t) => t,
            None => return "???".to_string(),
        };
        let precision = date.precision;
        let mut result = String::new();
        let mut bce_suffix = String::new();

        if year < 0 {
            bce_suffix = " B.C.E.".to_string();
        }

        if precision <= 9 {
            if !no_prefix {
                result.push_str("in ");
            }
            result.push_str(year_str);
        } else if precision == 10 {
            if !no_prefix {
                result.push_str("in ");
            }
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{} {}", month_label, year_str));
        } else {
            if !no_prefix {
                result.push_str("on ");
            }
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{} {}, {}", month_label, day, year_str));
        }

        result.push_str(&bce_suffix);
        result
    }

    /// Push a date range (from X until Y) in English.
    fn push_date_range(&self, state: &mut LongDescState, item: &DatedItem) {
        if let Some(ref from) = item.date_from {
            state.push_text("from ");
            state.push_text(&self.render_date(from, true));
            state.push_text(" ");
        }
        if let Some(ref to) = item.date_to {
            state.push_text("until ");
            state.push_text(&self.render_date(to, true));
            state.push_text(" ");
        }
    }
}
