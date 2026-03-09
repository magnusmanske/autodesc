use serde_json::Value;

use crate::short_desc::ShortDescription;
use crate::wikidata::WikiData;

use super::*;

const CFG: LangConfig = LangConfig {
    month_labels: [
        "",
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ],
    pronoun_subject_male: "Il",
    pronoun_possessive_male: "Son",
    pronoun_subject_female: "Elle",
    pronoun_possessive_female: "Son",
    be_present_singular: "est",
    be_past_singular: "était",
    pronoun_subject_neutral: "Il",
    pronoun_possessive_neutral: "Son",
    be_present_neutral: "est",
    be_past_neutral: "était",
};

/// Separator for French lists: "a, b et c".
fn get_sep_after_fr(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if (pos == 0 && len == 2) || (len == pos + 2) {
        " et "
    } else {
        ", "
    }
}

pub(super) struct LangFr;

impl LangGenerator for LangFr {
    fn add_first_sentence(
        &self,
        state: &mut LongDescState,
        q: &str,
        claims: &Value,
        _sd: &ShortDescription,
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

        // French: "est un/une" regardless of alive/dead
        let article = if state.is_male { "un " } else { "une " };
        state.push_text(&format!("est {}", article));

        // In French: occupations come before nationalities (use P2521/P3321 gendered label when available)
        let occupations = get_claim_item_ids(claims, "P106");
        for (k, occ_q) in occupations.iter().enumerate() {
            let sep = get_sep_after_fr(occupations.len(), k);
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

        // Nationalities
        let nationalities = get_claim_item_ids(claims, "P27");
        for (k, country_q) in nationalities.iter().enumerate() {
            let country_label = wd
                .get_item(country_q)
                .map(|i| i.get_label(Some(&state.lang)))
                .unwrap_or_default();
            if k > 0 {
                state.push_text("-");
            }
            let nat = if k > 0 {
                country_label.to_lowercase()
            } else {
                country_label
            };
            state.push_text(&nat);
            state.push_text(" ");
        }

        state.push_text(". ");

        let has_first_sentence = !nationalities.is_empty() || !occupations.is_empty();
        if !has_first_sentence {
            state.fragments.truncate(initial_len);
        }

        // Période de travail (P1317: floruit, P2031: début, P2032: fin)
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
            let active_adj = if state.is_female { "active" } else { "actif" };
            let bare_year =
                |d: &WdDate| parse_time(&d.time).map(|(_, y, _, _)| y.to_string()).unwrap_or_default();
            if let Some(ref date) = floruit {
                state.push_text(&format!("{} était {} vers {}",subj, active_adj, bare_year(date)));
            } else if let (Some(from), Some(to)) = (work_start.as_ref(), work_end.as_ref()) {
                state.push_text(&format!(
                    "{} était {} de {} à {}",
                    subj,
                    active_adj,
                    bare_year(from),
                    bare_year(to)
                ));
            } else if let Some(ref from) = work_start {
                state.push_text(&format!(
                    "{} était {} à partir de {}",
                    subj,
                    active_adj,
                    bare_year(from)
                ));
            } else if let Some(ref to) = work_end {
                state.push_text(&format!("{} était {} ", subj, active_adj));
                state.push_text(&self.render_date(to, true));
            }
            state.push_text(". ");
        }

        // Significant events
        let sig_events = get_claim_item_ids(claims, "P793");
        if !sig_events.is_empty() {
            let subj = state.pronoun_subject(&CFG);
            state.push_text(&format!("{} a joué un rôle important dans ", subj));
            for (k, ev_q) in sig_events.iter().enumerate() {
                state.push_item(ev_q, "", get_sep_after_fr(sig_events.len(), k));
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
        let born = if state.is_male { "est né" } else { "est née" };
        state.push_text(&format!("{} {} ", subj, born));

        if let Some(name) = &birthname {
            state.push_text(&format!("<i>{}</i> ", name));
        }

        if let Some(claim) = birthdate
            && let Some(date) = extract_claim_date(claim) {
                state.push_text(&self.render_date(&date, false));
                state.push_text(" ");
            }

        if let Some(ref place_q) = birthplace {
            state.push_item(place_q, "à ", " ");
        }

        // Parents
        let father = get_first_claim_item(claims, "P22");
        let mother = get_first_claim_item(claims, "P25");
        if father.is_some() || mother.is_some() {
            let child_of = if state.is_male {
                ". Il est le fils de "
            } else {
                ". Elle est la fille de "
            };
            state.push_text(child_of);
            if let Some(ref f) = father {
                state.push_item(f, "", " ");
            }
            if father.is_some() && mother.is_some() {
                state.push_text("et ");
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
        let is_dead = state.is_dead;

        // Education (P69)
        let alma = get_dated_items(claims, "P69", &[]);
        if !alma.is_empty() {
            state.push_text(&format!("{} a étudié à ", subj));
            for (k, item) in alma.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_fr(alma.len(), k));
            }
            state.push_text(". ");
        }

        // Field of work
        let mut fields: Vec<String> = get_claim_item_ids(claims, "P136");
        fields.extend(get_claim_item_ids(claims, "P101"));
        if !fields.is_empty() {
            let verb = if is_dead { "comprenait" } else { "comprend" };
            state.push_text(&format!("Son domaine de travail {} ", verb));
            for (k, q) in fields.iter().enumerate() {
                state.push_item(q, "", get_sep_after_fr(fields.len(), k));
            }
            state.push_text(". ");
        }

        // Position held (P39)
        let positions = get_dated_items(claims, "P39", &["P642"]);
        if !positions.is_empty() {
            let verb = if is_dead { "était " } else { "est/était " };
            state.push_text(&format!("{} {}", subj, verb));
            for (k, item) in positions.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(of_items) = item.qualifier_items.get("P642")
                    && let Some(of_q) = of_items.first() {
                        state.push_item(of_q, "pour ", " ");
                    }
                state.push_text(get_sep_after_fr(positions.len(), k));
            }
            state.push_text(". ");
        }

        // Member of (P463)
        let members = get_dated_items(claims, "P463", &[]);
        if !members.is_empty() {
            let verb = if is_dead { "était " } else { "est/était " };
            state.push_text(&format!("{} {}membre de ", subj, verb));
            for (k, item) in members.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                state.push_text(get_sep_after_fr(members.len(), k));
            }
            state.push_text(". ");
        }

        // Employers (P108)
        let employers = get_dated_items(claims, "P108", &["P794"]);
        if !employers.is_empty() {
            state.push_text(&format!("{} a travaillé pour ", subj));
            for (k, item) in employers.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                self.push_date_range(state, item);
                if let Some(job_items) = item.qualifier_items.get("P794")
                    && let Some(job_q) = job_items.first() {
                        state.push_item(job_q, "en tant que ", " ");
                    }
                let sep = get_sep_after_fr(employers.len(), k);
                if k + 1 < employers.len() {
                    state.push_text(&format!("{}pour ", sep));
                } else {
                    state.push_text(sep);
                }
            }
            state.push_text(". ");
        }

        // Notable works (P800)
        let notable_works = get_claim_item_ids(claims, "P800");
        if !notable_works.is_empty() {
            let verb = if is_dead {
                "comprenaient"
            } else {
                "comprennent"
            };
            state.push_text(&format!("Ses œuvres notables {} ", verb));
            for (k, q) in notable_works.iter().enumerate() {
                state.push_item(q, "", get_sep_after_fr(notable_works.len(), k));
            }
            state.push_text(". ");
        }

        state.push_newline();
    }

    fn add_family_text(&self, state: &mut LongDescState, claims: &Value) {
        let subj = state.pronoun_subject(&CFG);

        // Spouses (P26)
        let spouses = get_dated_items(claims, "P26", &[]);
        if !spouses.is_empty() {
            state.push_text(&format!("{} a épousé ", subj));
            for (k, item) in spouses.iter().enumerate() {
                state.push_item(&item.q, "", " ");
                if let Some(ref from) = item.date_from {
                    state.push_text(&self.render_date(from, false));
                    state.push_text(" ");
                }
                if let Some(ref to) = item.date_to {
                    state.push_text("(mariés ");
                    state.push_text(&self.render_date(to, true));
                    state.push_text(") ");
                }
                state.push_text(get_sep_after_fr(spouses.len(), k));
            }
            state.push_text(". ");
        }

        // Children (P40)
        let children = get_claim_item_ids(claims, "P40");
        if !children.is_empty() {
            let parent_of = if state.is_male {
                "Il est le père de "
            } else {
                "Elle est la mère de "
            };
            state.push_text(parent_of);
            for (k, q) in children.iter().enumerate() {
                state.push_item(q, "", get_sep_after_fr(children.len(), k));
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
            let died = if state.is_male {
                "est mort"
            } else {
                "est morte"
            };
            state.push_text(&format!("{} {} ", subj, died));

            if has_deathcause {
                let causes = get_claim_item_ids(claims, "P509");
                push_simple_list(state, &causes, "de ", " ", get_sep_after_fr);
            }

            if has_killer {
                let killers = get_claim_item_ids(claims, "P157");
                push_simple_list(state, &killers, "par ", " ", get_sep_after_fr);
            }

            if let Some(claim) = deathdate
                && let Some(date) = extract_claim_date(claim) {
                    state.push_text(&self.render_date(&date, false));
                    state.push_text(" ");
                }

            if let Some(ref place_q) = deathplace {
                state.push_item(place_q, "à ", " ");
            }

            state.push_text(". ");
        }

        // Burial place (P119)
        if let Some(burial_q) = get_first_claim_item(claims, "P119") {
            let subj = state.pronoun_subject(&CFG);
            let buried = if state.is_male {
                "est enterré à "
            } else {
                "est enterrée à "
            };
            state.push_item(&burial_q, &format!("{} {}", subj, buried), ". ");
        }
    }
}

impl LangFr {
    /// Render a Wikidata date as a French string.
    /// `jusque`: when true, use "jusqu'en"/"jusqu'au" instead of "en"/"le".
    fn render_date(&self, date: &WdDate, jusque: bool) -> String {
        let (year, year_str, month, day) = match parse_time(&date.time) {
            Some(t) => t,
            None => return "???".to_string(),
        };
        let precision = date.precision;
        let mut result = String::new();

        if precision <= 9 {
            result.push_str(if jusque { "jusqu'en " } else { "en " });
            result.push_str(year_str);
        } else if precision == 10 {
            result.push_str(if jusque { "jusqu'en " } else { "en " });
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{} {}", month_label, year_str));
        } else {
            result.push_str(if jusque { "jusqu'au " } else { "le " });
            let month_label = CFG.month_labels.get(month as usize).unwrap_or(&"");
            result.push_str(&format!("{} {} {}", day, month_label, year_str));
        }

        if year < 0 {
            result.push_str(" av. J.-C.");
        }

        result
    }

    /// Push a date range in French.
    fn push_date_range(&self, state: &mut LongDescState, item: &DatedItem) {
        if let Some(ref from) = item.date_from {
            state.push_text(&self.render_date(from, false));
            state.push_text(" ");
        }
        if let Some(ref to) = item.date_to {
            state.push_text(&self.render_date(to, true));
            state.push_text(" ");
        }
    }
}
