use serde_json::Value;
use wikimisc::escape;

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
        let label = escape::html_text(&get_main_title_label(q, wd, &state.lang)).into_owned();
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
                .get_item_str(country_q)
                .map(|i| i.get_label(Some(&state.lang)))
                .unwrap_or_default();
            let nationality =
                sd.get_nationality_from_country(&country_label, Some(country_q), &state.lang, wd);
            if k > 0 {
                state.push_text("-");
            }
            let nat = if k > 0 {
                nationality.to_lowercase()
            } else {
                nationality
            };
            state.push_text(&escape::html_text(&nat));
            if k + 1 == nationalities.len() {
                state.push_text(" ");
            }
        }

        // Occupations (use P2521/P3321 gendered label when available)
        let occupations = get_claim_item_ids(claims, "P106");
        for (k, occ_q) in occupations.iter().enumerate() {
            let sep = get_sep_after_en(occupations.len(), k);
            if (state.is_female || state.is_male)
                && let Some(gendered) = wd
                    .get_item_str(occ_q)
                    .and_then(|i| i.get_gendered_label(&state.lang, state.is_female))
            {
                state.push_text(&escape::html_text(&gendered));
                state.push_text(sep);
                continue;
            }
            state.push_item(occ_q, "", sep);
        }

        state.push_text(". ");

        let has_first_sentence = !nationalities.is_empty() || !occupations.is_empty();
        if !has_first_sentence {
            state.fragments.truncate(initial_len);
        }

        // Work period (P1317: floruit, P2031: work start, P2032: work end)
        let floruit = claims
            .get("P1317")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(extract_claim_date);
        let work_start = claims
            .get("P2031")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(extract_claim_date);
        let work_end = claims
            .get("P2032")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(extract_claim_date);
        if floruit.is_some() || work_start.is_some() || work_end.is_some() {
            let subj = state.pronoun_subject(&CFG);
            let poss = state.pronoun_possessive(&CFG);
            if let Some(ref date) = floruit {
                state.push_text(&format!("{} was professionally active around ", subj));
                state.push_text(&self.render_date(date, true));
            } else if let (Some(from), Some(to)) = (work_start.as_ref(), work_end.as_ref()) {
                state.push_text(&format!("{} professional career spanned from ", poss));
                state.push_text(&self.render_date(from, true));
                state.push_text(" to ");
                state.push_text(&self.render_date(to, true));
            } else if let Some(ref from) = work_start {
                state.push_text(&format!("{} was professionally active from ", subj));
                state.push_text(&self.render_date(from, true));
            } else if let Some(ref to) = work_end {
                state.push_text(&format!("{} was professionally active until ", subj));
                state.push_text(&self.render_date(to, true));
            }
            state.push_text(". ");
        }

        // Significant events (P793). On a person these are typically life
        // events such as a wedding, funeral or burial; phrasing them as "played
        // a role in" invents an agent role. Use a possessive instead, and only
        // include events whose point in time (P585) is known.
        if let Some(events) = claims.get("P793").and_then(Value::as_array) {
            let poss = state.pronoun_possessive(&CFG);
            let mut written = false;
            for claim in events {
                let Some(event_q) = get_claim_item_id(claim) else {
                    continue;
                };
                let Some(date) = claim
                    .get("qualifiers")
                    .and_then(|qs| extract_date_qualifier(qs, "P585"))
                else {
                    continue;
                };
                if written {
                    state.push_text(" ");
                }
                written = true;
                state.push_text(&format!("{} ", poss));
                state.push_item(&event_q, "", " took place ");
                state.push_text(&self.render_date(&date, false));
                state.push_text(". ");
            }
            if written {
                state.push_newline();
            }
        }

        state.push_newline();
    }

    fn add_birth_text(&self, state: &mut LongDescState, claims: &Value) {
        let birthdate = claims
            .get("P569")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first());
        let birthplace = get_first_claim_item(claims, "P19");
        let birthname = get_first_claim_string(claims, "P513");

        if birthdate.is_none() && birthplace.is_none() && birthname.is_none() {
            return;
        }

        let subj = state.pronoun_subject(&CFG);
        let be_past = state.be_past(&CFG);
        state.push_text(&format!("{} {} born ", subj, be_past));

        if let Some(name) = &birthname {
            state.push_text(&format!("<i>{}</i> ", escape::html_text(name)));
        }

        if let Some(claim) = birthdate
            && let Some(date) = extract_claim_date(claim)
        {
            state.push_text(&self.render_date(&date, false));
            state.push_text(" ");
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
                if let Some(of_items) = item.qualifier_items.get("P642")
                    && let Some(of_q) = of_items.first()
                {
                    state.push_item(of_q, "for ", " ");
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
                if let Some(job_items) = item.qualifier_items.get("P794")
                    && let Some(job_q) = job_items.first()
                {
                    state.push_item(job_q, "as ", " ");
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
                    state.push_text(&self.render_date(to, true));
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
        let deathdate = claims
            .get("P570")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first());
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

            if let Some(claim) = deathdate
                && let Some(date) = extract_claim_date(claim)
            {
                state.push_text(&self.render_date(&date, false));
                state.push_text(" ");
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
    /// `no_prefix`: when true, omit the "in"/"on" preposition. It also
    /// suppresses the circa marker, because callers that pass `true` already
    /// supply their own preposition or wording.
    fn render_date(&self, date: &WdDate, no_prefix: bool) -> String {
        let Some(kind) = date.kind() else {
            return "???".to_string();
        };
        let circa = date.circa && !no_prefix;

        let value = match kind {
            DateKind::Day { year, month, day } => {
                let month_label = CFG.month_labels.get(usize::from(month)).unwrap_or(&"");
                format!(
                    "{} {}, {}{}",
                    month_label,
                    day,
                    year.unsigned_abs(),
                    bce_suffix(year)
                )
            }
            DateKind::Month { year, month } => {
                let month_label = CFG.month_labels.get(usize::from(month)).unwrap_or(&"");
                format!(
                    "{} {}{}",
                    month_label,
                    year.unsigned_abs(),
                    bce_suffix(year)
                )
            }
            DateKind::Year { year } => format!("{}{}", year.unsigned_abs(), bce_suffix(year)),
            DateKind::Decade { start, bce } => {
                format!("the {}s{}", start, if bce { " B.C.E." } else { "" })
            }
            DateKind::Century { number, bce } => format!(
                "the {} century{}",
                en_ordinal(number),
                if bce { " B.C.E." } else { "" }
            ),
            DateKind::Millennium { number, bce } => format!(
                "the {} millennium{}",
                en_ordinal(number),
                if bce { " B.C.E." } else { "" }
            ),
        };

        let prefix = if no_prefix {
            ""
        } else if circa {
            "around "
        } else if matches!(kind, DateKind::Day { .. }) {
            "on "
        } else {
            "in "
        };

        format!("{}{}", prefix, value)
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

/// The suffix English adds to a year before the common era.
fn bce_suffix(year: i64) -> &'static str {
    if year < 0 { " B.C.E." } else { "" }
}

/// English ordinal numerals: 1st, 2nd, 3rd, 4th, 11th, 12th, 13th, 21st, …
fn en_ordinal(number: i64) -> String {
    let suffix = match (number % 100, number % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{}{}", number, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wd(time: &str, precision: u64, circa: bool) -> WdDate {
        WdDate {
            time: time.to_string(),
            precision,
            circa,
        }
    }

    #[test]
    fn render_date_uses_precision() {
        let lang = LangEn;
        assert_eq!(
            lang.render_date(&wd("+1940-00-00T00:00:00Z", 8, false), false),
            "in the 1940s"
        );
        assert_eq!(
            lang.render_date(&wd("+1940-00-00T00:00:00Z", 7, false), false),
            "in the 20th century"
        );
        assert_eq!(
            lang.render_date(&wd("+1940-00-00T00:00:00Z", 6, false), false),
            "in the 2nd millennium"
        );
        assert_eq!(
            lang.render_date(&wd("+1960-00-00T00:00:00Z", 9, false), false),
            "in 1960"
        );
    }

    #[test]
    fn render_date_combines_circa_and_precision() {
        let lang = LangEn;
        assert_eq!(
            lang.render_date(&wd("+1960-00-00T00:00:00Z", 9, true), false),
            "around 1960"
        );
        assert_eq!(
            lang.render_date(&wd("+1940-00-00T00:00:00Z", 8, true), false),
            "around the 1940s"
        );
    }

    #[test]
    fn render_date_keeps_exact_day_format() {
        let lang = LangEn;
        let date = wd("+1952-03-11T00:00:00Z", 11, false);
        assert_eq!(lang.render_date(&date, false), "on March 11, 1952");
        assert_eq!(lang.render_date(&date, true), "March 11, 1952");
    }

    #[test]
    fn ordinal_suffixes() {
        assert_eq!(en_ordinal(1), "1st");
        assert_eq!(en_ordinal(2), "2nd");
        assert_eq!(en_ordinal(3), "3rd");
        assert_eq!(en_ordinal(11), "11th");
        assert_eq!(en_ordinal(12), "12th");
        assert_eq!(en_ordinal(13), "13th");
        assert_eq!(en_ordinal(21), "21st");
    }
}
