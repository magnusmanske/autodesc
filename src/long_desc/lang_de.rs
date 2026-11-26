use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;
use wikimisc::escape;

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

/// German nationality adjective stems keyed by Wikidata country Q-ID.
/// Loaded once from the embedded JSON file.
fn de_adj_base_for_country(country_q: &str) -> Option<&'static str> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/de_nationality_adjectives.json"))
            .unwrap_or_default()
    });
    map.get(country_q).map(String::as_str)
}

/// Separator for German lists: "a, b und c".
fn get_sep_after_de(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if (pos == 0 && len == 2) || (len == pos + 2) {
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

        // German Wikipedia puts birth and death details right after the name,
        // using the genealogical signs: (* 21. März 1685 in Eisenach; † …).
        let has_bio = self.push_biographical_parenthetical(state, claims);

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
                .get_item_str(country_q)
                .map(|i| i.get_label(Some(&state.lang)))
                .unwrap_or_default();

            // Get adjective base in priority order:
            // 1. Static Q-ID table (most reliable)
            // 2. P1549 lowercase value (Wikidata adjective form, if present)
            // 3. Noun fallback from get_nationality_from_country (used as-is)
            let suffix = if state.is_male { "er" } else { "e" };
            let nationality = de_adj_base_for_country(country_q)
                .map(|s| s.to_string())
                .or_else(|| {
                    wd.get_item_str(country_q)
                        .and_then(|i| i.get_demonym_adjective_base("de"))
                })
                .map(|base| format!("{}{}", base, suffix))
                .unwrap_or_else(|| {
                    sd.get_nationality_from_country(
                        &country_label,
                        Some(country_q),
                        &state.lang,
                        wd,
                    )
                });

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
            let sep = get_sep_after_de(occupations.len(), k);
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
        // If the intro sentence was dropped, the parenthetical went with it;
        // the birth/death sections must then render the dates themselves.
        state.bio_in_lead = has_bio && has_first_sentence;

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

        // Significant events (P793) are not rendered: treating the subject as
        // an agent of a life event (wedding, funeral, burial) produces wrong
        // prose, and there is no reliable generic phrasing.

        state.push_newline();
    }

    fn add_birth_text(&self, state: &mut LongDescState, claims: &Value) {
        let birthdate = claims
            .get("P569")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first());
        let birthplace = get_first_claim_item(claims, "P19");
        let birthname = get_first_claim_string(claims, "P513");
        let father = get_first_claim_item(claims, "P22");
        let mother = get_first_claim_item(claims, "P25");
        let has_parents = father.is_some() || mother.is_some();
        let has_date_or_place = birthdate.is_some() || birthplace.is_some();

        // Date and place may already be in the opening parenthetical; without
        // any remaining detail there is nothing left to say here.
        if birthname.is_none() && !has_parents && (state.bio_in_lead || !has_date_or_place) {
            return;
        }

        let subj = state.pronoun_subject(&CFG);

        if let Some(name) = &birthname {
            state.push_text(&format!(
                "{} wurde als <i>{}</i>",
                subj,
                escape::html_text(name)
            ));
            if !state.bio_in_lead {
                state.push_text(" ");
                self.push_birth_date_and_place(state, birthdate, birthplace.as_deref());
            }
            state.push_text(" geboren.");
            if has_parents {
                let possessive = if state.is_female {
                    "Ihre"
                } else if state.is_male {
                    "Seine"
                } else {
                    "Seine/Ihre"
                };
                state.push_text(&format!(" {} Eltern waren ", possessive));
                self.push_parents(state, father.as_deref(), mother.as_deref());
                state.push_text(".");
            }
        } else {
            state.push_text(&format!("{} wurde ", subj));
            if !state.bio_in_lead {
                self.push_birth_date_and_place(state, birthdate, birthplace.as_deref());
            }
            if has_parents {
                let child_of = if state.is_male {
                    "als Sohn von "
                } else if state.is_female {
                    "als Tochter von "
                } else {
                    "als Kind von "
                };
                state.push_text(child_of);
                self.push_parents(state, father.as_deref(), mother.as_deref());
            }
            state.push_text("geboren.");
        }

        state.push_text(" ");
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
            state.push_text(&format!(
                "Zu {}en Kindern z\u{00e4}hlen ",
                poss.to_lowercase()
            ));
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

        // If the death data is already in the opening parenthetical, only
        // render what adds something (cause of death, killer).
        let render_death_sentence = has_deathcause
            || has_killer
            || (!state.bio_in_lead && (deathdate.is_some() || deathplace.is_some()));

        if render_death_sentence {
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

            if !state.bio_in_lead {
                if let Some(claim) = deathdate
                    && let Some(date) = extract_claim_date(claim)
                {
                    state.push_text(&self.render_date(&date, false));
                    state.push_text(" ");
                }

                if let Some(ref place_q) = deathplace {
                    state.push_item(place_q, "in ", " ");
                }
            }

            state.push_text(". ");
        }

        // Burial place (P119)
        if let Some(burial_q) = get_first_claim_item(claims, "P119") {
            let subj = state.pronoun_subject(&CFG);
            state.push_item(&burial_q, &format!("{} wurde in ", subj), " begraben. ");
        }
    }
}

impl LangDe {
    /// Push the German Wikipedia–style birth/death parenthetical directly
    /// after the bold name, e.g.
    /// `(* 21. März 1685 in Eisenach; † 28. Juli 1750 in Leipzig)`.
    /// Returns `true` if anything was written.
    fn push_biographical_parenthetical(&self, state: &mut LongDescState, claims: &Value) -> bool {
        let birthdate = first_claim_date(claims, "P569");
        let deathdate = first_claim_date(claims, "P570");
        let birthplace = get_first_claim_item(claims, "P19");
        let deathplace = get_first_claim_item(claims, "P20");

        if birthdate.is_none()
            && deathdate.is_none()
            && birthplace.is_none()
            && deathplace.is_none()
        {
            return false;
        }

        state.push_text("(");
        let mut wrote = false;

        if birthdate.is_some() || birthplace.is_some() {
            state.push_text("* ");
            self.push_date_and_place(state, birthdate.as_ref(), birthplace.as_deref(), false);
            wrote = true;
        }

        if deathdate.is_some() || deathplace.is_some() {
            if wrote {
                state.push_text("; ");
            }
            state.push_text("† ");
            let same_place = birthplace.is_some() && birthplace == deathplace;
            self.push_date_and_place(state, deathdate.as_ref(), deathplace.as_deref(), same_place);
            wrote = true;
        }

        state.push_text(") ");
        wrote
    }

    /// Push a date followed by its place ("in …"), or "ebenda" when the
    /// death place equals the birth place.
    fn push_date_and_place(
        &self,
        state: &mut LongDescState,
        date: Option<&WdDate>,
        place: Option<&str>,
        same_place_as_birth: bool,
    ) {
        let mut wrote = false;
        if let Some(date) = date {
            state.push_text(&self.render_biographical_date(date));
            wrote = true;
        }
        if same_place_as_birth {
            if wrote {
                state.push_text(" ");
            }
            state.push_text("ebenda");
        } else if let Some(q) = place {
            if wrote {
                state.push_text(" ");
            }
            state.push_item(q, "in ", "");
        }
    }

    /// Render a date for the biographical parenthetical: no "am"/"im"
    /// preposition, but "um" for an approximate (circa) date.
    fn render_biographical_date(&self, date: &WdDate) -> String {
        let rendered = self.render_date(date, true);
        if !date.circa {
            return rendered;
        }
        match date.kind() {
            Some(DateKind::Decade { .. }) => format!("um die {}", rendered),
            Some(DateKind::Century { .. }) | Some(DateKind::Millennium { .. }) => {
                format!("um das {}", rendered)
            }
            _ => format!("um {}", rendered),
        }
    }

    /// Push the birth date and place for the prose fallback (used when the
    /// parenthetical is not part of the output).
    fn push_birth_date_and_place(
        &self,
        state: &mut LongDescState,
        birthdate: Option<&Value>,
        birthplace: Option<&str>,
    ) {
        if let Some(claim) = birthdate
            && let Some(date) = extract_claim_date(claim)
        {
            state.push_text(&self.render_date(&date, false));
            state.push_text(" ");
        }
        if let Some(place_q) = birthplace {
            state.push_item(place_q, "in ", " ");
        }
    }

    /// Push "A und B" for a pair of parents.
    fn push_parents(&self, state: &mut LongDescState, father: Option<&str>, mother: Option<&str>) {
        if let Some(f) = father {
            state.push_item(f, "", " ");
        }
        if father.is_some() && mother.is_some() {
            state.push_text("und ");
        }
        if let Some(m) = mother {
            state.push_item(m, "", " ");
        }
    }

    /// Render a Wikidata date as a German string.
    /// `no_prefix`: when true, omit the "am"/"im" preposition. It also
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
                let article = if circa { "den " } else { "" };
                format!(
                    "{}{}. {} {}{}",
                    article,
                    day,
                    month_label,
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
                let suffix = if bce { " v. Chr." } else { "" };
                if circa {
                    format!("die {}er Jahre{}", start, suffix)
                } else if no_prefix {
                    format!("{}er Jahre{}", start, suffix)
                } else {
                    format!("den {}er Jahren{}", start, suffix)
                }
            }
            DateKind::Century { number, bce } => {
                let suffix = if bce { " v. Chr." } else { "" };
                let article = if circa { "das " } else { "" };
                format!("{}{}. Jahrhundert{}", article, number, suffix)
            }
            DateKind::Millennium { number, bce } => {
                let suffix = if bce { " v. Chr." } else { "" };
                let article = if circa { "das " } else { "" };
                format!("{}{}. Jahrtausend{}", article, number, suffix)
            }
        };

        let prefix = if no_prefix {
            ""
        } else if circa {
            "um "
        } else {
            match kind {
                DateKind::Day { .. } => "am ",
                DateKind::Month { .. } => "im ",
                DateKind::Year { .. } => "im Jahr ",
                DateKind::Decade { .. } => "in ",
                DateKind::Century { .. } | DateKind::Millennium { .. } => "im ",
            }
        };

        format!("{}{}", prefix, value)
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

/// Extract the first date value of a property, if present.
fn first_claim_date(claims: &Value, prop: &str) -> Option<WdDate> {
    claims
        .get(prop)
        .and_then(Value::as_array)
        .and_then(|claims| claims.first())
        .and_then(extract_claim_date)
}

/// The suffix German adds to a year before the common era.
fn bce_suffix(year: i64) -> &'static str {
    if year < 0 { " v. Chr." } else { "" }
}
