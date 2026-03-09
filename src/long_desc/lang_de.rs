use serde_json::Value;

use crate::short_desc::ShortDescription;
use crate::wikidata::WikiData;

use super::*;

const CFG: LangConfig = LangConfig {
    month_labels: [
        "",
        "Januar",
        "Februar",
        "M\u{00e4}rz",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    pronoun_subject_male: "Er",
    pronoun_possessive_male: "Sein",
    pronoun_subject_female: "Sie",
    pronoun_possessive_female: "Ihr",
    pronoun_subject_neutral: "Er/Sie",
    pronoun_possessive_neutral: "Sein/Ihr",
    be_present_singular: "ist",
    be_past_singular: "war",
    be_present_neutral: "ist",
    be_past_neutral: "war",
};

/// Separator for German lists: "a, b und c".
fn get_sep_after_de(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if pos == 0 && len == 2 {
        " und "
    } else if len == pos + 2 {
        " und "
    } else {
        ", "
    }
}

pub(super) struct LangDe;

impl LangGenerator for LangDe {
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

        let be = if state.is_dead { "war" } else { "ist" };
        let article = if state.is_male {
            "ein "
        } else if state.is_female {
            "eine "
        } else {
            "ein(e) "
        };
        state.push_text(&format!("{} {}", be, article));

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
            let nat = if k > 0 {
                nationality.to_lowercase()
            } else {
                nationality
            };
            state.push_text(&nat);
            if k + 1 == nationalities.len() {
                state.push_text(" ");
            }
        }

        // Occupations (use P2521/P3321 gendered label when available)
        let occupations = get_claim_item_ids(claims, "P106");
        for (k, occ_q) in occupations.iter().enumerate() {
            let sep = get_sep_after_de(occupations.len(), k);
            if (state.is_female || state.is_male)
                && let Some(gendered) = wd
                    .get_item(occ_q)
                    .and_then(|i| i.get_gendered_label(&state.lang, state.is_female))
                {
                    state.push_text(&gendered);
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

        // Schaffensperiode (P1317: floruit, P2031: Beginn, P2032: Ende)
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
                state.push_text(&format!("{} war beruflich t\u{00e4}tig um ", subj));
                state.push_text(&self.render_date(date, true));
            } else if let (Some(from), Some(to)) = (work_start.as_ref(), work_end.as_ref()) {
                state.push_text(&format!(
                    "{}e berufliche Laufbahn erstreckte sich von ",
                    poss
                ));
                state.push_text(&self.render_date(from, true));
                state.push_text(" bis ");
                state.push_text(&self.render_date(to, true));
            } else if let Some(ref from) = work_start {
                state.push_text(&format!("{} war beruflich t\u{00e4}tig ab ", subj));
                state.push_text(&self.render_date(from, true));
            } else if let Some(ref to) = work_end {
                state.push_text(&format!("{} war beruflich t\u{00e4}tig bis ", subj));
                state.push_text(&self.render_date(to, true));
            }
            state.push_text(". ");
        }

        // Significant events
        let sig_events = get_claim_item_ids(claims, "P793");
        if !sig_events.is_empty() {
            let subj = state.pronoun_subject(&CFG);
            state.push_text(&format!("{} spielte eine Rolle bei ", subj));
            for (k, ev_q) in sig_events.iter().enumerate() {
                state.push_item(ev_q, "", get_sep_after_de(sig_events.len(), k));
            }
            state.push_text(".");
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
        state.push_text(&format!("{} wurde ", subj));

        if let Some(name) = &birthname {
            state.push_text(&format!("als <i>{}</i> ", name));
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
            let child_of = if state.is_male {
                "als Sohn von "
            } else if state.is_female {
                "als Tochter von "
            } else {
                "als Kind von "
            };
            state.push_text(child_of);
            if let Some(ref f) = father {
                state.push_item(f, "", " ");
            }
            if father.is_some() && mother.is_some() {
                state.push_text("und ");
            }
            if let Some(ref m) = mother {
                state.push_item(m, "", " ");
            }
        }

        state.push_text("geboren. ");
        state.push_newline();
    }

    fn add_work_text(&self, state: &mut LongDescState, claims: &Value) {
        let subj = state.pronoun_subject(&CFG);
        let poss = state.pronoun_possessive(&CFG);
        let is_dead = state.is_dead;

        // Education (P69)
        let alma = get_dated_items(claims, "P69", &[]);
        if !alma.is_empty() {
            state.push_text(&format!("{} studierte an der ", subj));
            for (k, item) in alma.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_de(alma.len(), k));
            }
            state.push_text(". ");
        }

        // Field of work (P136, P101)
        let mut fields: Vec<String> = get_claim_item_ids(claims, "P136");
        fields.extend(get_claim_item_ids(claims, "P101"));
        if !fields.is_empty() {
            let verb = if is_dead { "umfasste" } else { "umfasst" };
            state.push_text(&format!("{}e Arbeitsgebiet {} ", poss, verb));
            for (k, q) in fields.iter().enumerate() {
                state.push_item(q, "", get_sep_after_de(fields.len(), k));
            }
            state.push_text(". ");
        }

        // Position held (P39) with qualifier P642 ("of")
        let positions = get_dated_items(claims, "P39", &["P642"]);
        if !positions.is_empty() {
            let verb = if is_dead { "war " } else { "ist/war " };
            state.push_text(&format!("{} {}", subj, verb));
            for (k, item) in positions.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(of_items) = item.qualifier_items.get("P642")
                    && let Some(of_q) = of_items.first()
                {
                    state.push_item(of_q, "f\u{00fc}r ", " ");
                }
                state.push_text(get_sep_after_de(positions.len(), k));
            }
            state.push_text(". ");
        }

        // Member of (P463)
        let members = get_dated_items(claims, "P463", &[]);
        if !members.is_empty() {
            let verb = if is_dead { "war " } else { "ist/war " };
            state.push_text(&format!("{} {}Mitglied von ", subj, verb));
            for (k, item) in members.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_de(members.len(), k));
            }
            state.push_text(". ");
        }

        // Employers (P108) with qualifier P794 ("as")
        let employers = get_dated_items(claims, "P108", &["P794"]);
        if !employers.is_empty() {
            state.push_text(&format!("{} arbeitete f\u{00fc}r ", subj));
            for (k, item) in employers.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(job_items) = item.qualifier_items.get("P794")
                    && let Some(job_q) = job_items.first()
                {
                    state.push_item(job_q, "als ", " ");
                }
                let sep = get_sep_after_de(employers.len(), k);
                if k + 1 < employers.len() {
                    state.push_text(&format!("{}f\u{00fc}r ", sep));
                } else {
                    state.push_text(sep);
                }
            }
            state.push_text(". ");
        }

        // Notable works (P800)
        let notable_works = get_claim_item_ids(claims, "P800");
        if !notable_works.is_empty() {
            let verb = if is_dead { "umfassten" } else { "umfassen" };
            state.push_text(&format!("{}e bedeutenden Werke {} ", poss, verb));
            for (k, q) in notable_works.iter().enumerate() {
                state.push_item(q, "", get_sep_after_de(notable_works.len(), k));
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
            state.push_text(&format!("{} heiratete ", subj));
            for (k, item) in spouses.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                if let Some(ref from) = item.date_from {
                    state.push_text(&self.render_date(from, false));
                    state.push_text(" ");
                }
                if let Some(ref to) = item.date_to {
                    state.push_text("(verheiratet bis ");
                    state.push_text(&self.render_date(to, true));
                    state.push_text(") ");
                }
                state.push_text(get_sep_after_de(spouses.len(), k));
            }
            state.push_text(". ");
        }

        // Children (P40)
        let children = get_claim_item_ids(claims, "P40");
        if !children.is_empty() {
            state.push_text(&format!("Zu {}en Kindern z\u{00e4}hlen ", poss.to_lowercase()));
            for (k, q) in children.iter().enumerate() {
                state.push_item(q, "", get_sep_after_de(children.len(), k));
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
            state.push_text(&format!("{} starb ", subj));

            if has_deathcause {
                let causes = get_claim_item_ids(claims, "P509");
                push_simple_list(state, &causes, "an ", " ", get_sep_after_de);
            }

            if has_killer {
                let killers = get_claim_item_ids(claims, "P157");
                push_simple_list(state, &killers, "durch ", " ", get_sep_after_de);
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
            state.push_item(
                &burial_q,
                &format!("{} wurde in ", subj),
                " begraben. ",
            );
        }
    }
}

impl LangDe {
    /// Render a Wikidata date as a German string.
    /// `no_prefix`: when true, omit the "am"/"im" preposition.
    fn render_date(&self, date: &WdDate, no_prefix: bool) -> String {
        let (year, year_str, month, day) = match parse_time(&date.time) {
            Some(t) => t,
            None => return "???".to_string(),
        };
        let precision = date.precision;
        let mut result = String::new();

        if precision <= 9 {
            if !no_prefix {
                result.push_str("im Jahr ");
            }
            result.push_str(year_str);
        } else if precision == 10 {
            if !no_prefix {
                result.push_str("im ");
            }
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{} {}", month_label, year_str));
        } else {
            if !no_prefix {
                result.push_str("am ");
            }
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{}. {} {}", day, month_label, year_str));
        }

        if year < 0 {
            result.push_str(" v. Chr.");
        }

        result
    }

    /// Push a date range (von X bis Y) in German.
    fn push_date_range(&self, state: &mut LongDescState, item: &DatedItem) {
        if let Some(ref from) = item.date_from {
            state.push_text("von ");
            state.push_text(&self.render_date(from, true));
            state.push_text(" ");
        }
        if let Some(ref to) = item.date_to {
            state.push_text("bis ");
            state.push_text(&self.render_date(to, true));
            state.push_text(" ");
        }
    }
}
