//! What a site asks of the programs that read it, for three separate uses:
//! search engines listing its pages, AI assistants reading a page to answer a
//! question, and AI companies training models on it. Each is a `[site]` switch
//! (`search_engines`, `ai_answers`, `ai_training`); `ai_policy` stays as a name
//! for one state of the three.
//!
//! Every output here is a request. A crawler that ignores robots.txt, a meta
//! tag or a reservation file can still read the site; only a login keeps a
//! page private.
//!
//! A site that sets none of the three switches keeps the robots.txt its
//! `ai_policy` has always produced. The fuller output (the robots meta tag,
//! the per-use crawler groups, `tdmrep.json`) begins when the owner sets a
//! switch. `llms.txt` follows the AI answers use in both forms, so a
//! `restricted` site stops publishing it.

use crate::build::served_path::ServedPath;

/// Crawlers that collect pages to train models. Order is part of the
/// published robots.txt and stays as sites have always had it.
const TRAINING_CRAWLERS: [&str; 8] = [
    "GPTBot",
    "Google-Extended",
    "ClaudeBot",
    "CCBot",
    "Bytespider",
    "meta-externalagent",
    "Amazonbot",
    "Applebot-Extended",
];

/// AI search indexes, which document robots.txt rather than `noindex`.
/// Googlebot and Bingbot are never blocked: they must read a page to see its
/// `noindex`.
const AI_SEARCH_CRAWLERS: [&str; 3] = ["OAI-SearchBot", "Claude-SearchBot", "PerplexityBot"];

/// Agents that fetch a page when a person asks an assistant a question.
const USER_FETCHERS: [&str; 3] = ["ChatGPT-User", "Claude-User", "Perplexity-User"];

/// Google-Extended also covers Gemini's grounding, so it joins the answers
/// group when training is allowed but answers are not.
const GOOGLE_EXTENDED: &str = "Google-Extended";

/// Which of the three uses the site allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uses {
    pub search: bool,
    pub answers: bool,
    pub training: bool,
}

/// The named states `[site].ai_policy` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Search and answers allowed, training asked not to. The default.
    Standard,
    Unrestricted,
    Restricted,
}

impl Preset {
    fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("standard") => Preset::Standard,
            Some("unrestricted") => Preset::Unrestricted,
            Some("restricted") => Preset::Restricted,
            // A typo like "unrestrcted" must not silently flip a site's policy.
            Some(other) => {
                log::warn!("Unknown [site].ai_policy {:?}; falling back to \"standard\"", other);
                Preset::Standard
            }
        }
    }

    pub fn uses(self) -> Uses {
        match self {
            Preset::Standard => Uses { search: true, answers: true, training: false },
            Preset::Unrestricted => Uses { search: true, answers: true, training: true },
            Preset::Restricted => Uses { search: false, answers: false, training: false },
        }
    }
}

/// The site's choice, resolved once per build from `[site]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiSettings {
    /// No switch is set: the files `ai_policy` has always produced.
    Preset(Preset),
    /// At least one switch is set; an unset one takes `ai_policy`'s value.
    Switches(Uses),
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings::Preset(Preset::Standard)
    }
}

impl AiSettings {
    pub fn resolve(
        ai_policy: Option<&str>,
        search_engines: Option<bool>,
        ai_answers: Option<bool>,
        ai_training: Option<bool>,
    ) -> Self {
        let preset = Preset::from_config(ai_policy);
        if search_engines.is_none() && ai_answers.is_none() && ai_training.is_none() {
            return AiSettings::Preset(preset);
        }
        let base = preset.uses();
        AiSettings::Switches(Uses {
            search: search_engines.unwrap_or(base.search),
            answers: ai_answers.unwrap_or(base.answers),
            training: ai_training.unwrap_or(base.training),
        })
    }

    /// The site's `[site]` keys, from its parsed config.
    pub fn from_config(cfg: Option<&crate::config::ConfigFile>) -> Self {
        let switch = |key: &str| cfg.and_then(|c| c.site_bool(key));
        Self::resolve(
            cfg.and_then(|c| c.site_str("ai_policy")),
            switch("search_engines"),
            switch("ai_answers"),
            switch("ai_training"),
        )
    }

    /// [`Self::from_config`] for a site folder; an unreadable config reads as
    /// no keys, which is what the build would have refused or defaulted to.
    pub fn for_folder(mp: &crate::moss_paths::MossPaths) -> Self {
        let text = crate::build::site_config::read_managed_toml_no_wait(&mp.config());
        let cfg = text.ok().flatten().and_then(|t| crate::config::ConfigFile::parse(&t).ok());
        Self::from_config(cfg.as_ref())
    }

    pub fn uses(self) -> Uses {
        match self {
            AiSettings::Preset(p) => p.uses(),
            AiSettings::Switches(u) => u,
        }
    }

    /// The switch state, or `None` for a site that set no switch.
    fn switches(self) -> Option<Uses> {
        match self {
            AiSettings::Switches(u) => Some(u),
            AiSettings::Preset(_) => None,
        }
    }

    /// robots.txt, ending with the Sitemap line unless search is asked not to.
    pub fn robots_txt(self, site_url: &str) -> String {
        let sitemap = format!(
            "Sitemap: {}{}\n",
            site_url.trim_end_matches('/'),
            crate::build::served_path::ServedPath::for_sitemap().to_relative_url()
        );
        let uses = match self {
            AiSettings::Preset(p) => return format!("{}\n{}", preset_block(p), sitemap),
            AiSettings::Switches(u) => u,
        };
        let say = |allowed: bool| if allowed { "allowed" } else { "asked not to" };
        let yn = |allowed: bool| if allowed { "yes" } else { "no" };
        let mut out = format!(
            "# This site's requests to the programs that read it. They are requests, not locks.\n\
             # Search engines: {}. AI answers: {}. AI training: {}.\n\n\
             User-agent: *\nContent-Signal: search={}, ai-input={}, ai-train={}\nAllow: /\n",
            say(uses.search), say(uses.answers), say(uses.training),
            yn(uses.search), yn(uses.answers), yn(uses.training),
        );
        let mut group = |comment: &str, agents: &[&str]| {
            out.push_str(&format!("\n# {comment}\n"));
            for agent in agents {
                out.push_str(&format!("User-agent: {agent}\n"));
            }
            out.push_str("Disallow: /\n");
        };
        if !uses.search {
            group(
                "Asked not to list this site in AI search. Search engines may still read pages, to see their noindex tag.",
                &AI_SEARCH_CRAWLERS,
            );
        }
        if !uses.answers {
            let mut agents = USER_FETCHERS.to_vec();
            if uses.training {
                agents.push(GOOGLE_EXTENDED);
            }
            group("Asked not to read pages to answer questions", &agents);
        }
        if !uses.training {
            group("Asked not to train on this site", &TRAINING_CRAWLERS);
        }
        if uses.search {
            out.push('\n');
            out.push_str(&sitemap);
        }
        out
    }

    /// The page's `<meta name="robots">` tag, if it needs one; the caller
    /// places it in its `<head>`. `page_noindex` is the page's own reason to
    /// stay out of search (a draft).
    pub fn robots_tag(self, page_noindex: bool) -> Option<String> {
        let uses = self.switches();
        let mut words: Vec<&str> = Vec::new();
        if page_noindex || uses.is_some_and(|u| !u.search) {
            words.push("noindex");
        }
        if uses.is_some_and(|u| !u.training) {
            words.extend(["noai", "noimageai"]);
        }
        (!words.is_empty()).then(|| format!("<meta name=\"robots\" content=\"{}\">", words.join(", ")))
    }

    /// Whether the site's whole text is published as `llms.txt`.
    pub fn publishes_llms_txt(self) -> bool {
        self.uses().answers
    }

    /// Addresses this choice stops serving on purpose: `llms.txt` while AI
    /// answers are asked not to, and `tdmrep.json` once no switch is set. The
    /// publish gate reads the disappearance of one as the owner's act.
    pub fn withdrawn(self) -> Vec<ServedPath> {
        let llms = (!self.publishes_llms_txt()).then(ServedPath::for_llms_txt);
        let tdmrep = self.tdmrep_json().is_none().then(ServedPath::for_tdmrep_json);
        llms.into_iter().chain(tdmrep).collect()
    }

    /// `/.well-known/tdmrep.json`, the W3C TDM Reservation Protocol's form of
    /// the EU text-and-data-mining reservation. Written whichever way the
    /// switch is set (`tdm-reservation` 1 or 0), so turning training back on
    /// never removes an address the site served.
    pub fn tdmrep_json(self) -> Option<String> {
        self.switches().map(|u| {
            format!("[\n  {{ \"location\": \"/\", \"tdm-reservation\": {} }}\n]\n", if u.training { 0 } else { 1 })
        })
    }
}

/// What a settings surface shows: the three uses as the build reads them, and
/// the files they produce, from this module's own generator.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct AiSettingsView {
    pub search: bool,
    pub answers: bool,
    pub training: bool,
    /// False while the site sets none of the three switches: its files are
    /// still the ones its `ai_policy` preset has always written.
    pub switches_set: bool,
    pub robots_txt: String,
    pub robots_tag: Option<String>,
    pub publishes_llms_txt: bool,
    pub tdmrep_json: Option<String>,
}

impl AiSettings {
    pub fn view(self, site_url: &str) -> AiSettingsView {
        let uses = self.uses();
        AiSettingsView {
            search: uses.search,
            answers: uses.answers,
            training: uses.training,
            switches_set: self.switches().is_some(),
            robots_txt: self.robots_txt(site_url),
            robots_tag: self.robots_tag(false),
            publishes_llms_txt: self.publishes_llms_txt(),
            tdmrep_json: self.tdmrep_json(),
        }
    }
}

/// The robots.txt body each preset has always written, byte for byte.
fn preset_block(preset: Preset) -> String {
    match preset {
        Preset::Unrestricted => {
            "User-agent: *\nContent-Signal: search=yes, ai-input=yes, ai-train=yes\nAllow: /\n".to_string()
        }
        Preset::Restricted => {
            "User-agent: *\nContent-Signal: search=no, ai-input=no, ai-train=no\nDisallow: /\n".to_string()
        }
        Preset::Standard => {
            let mut block =
                String::from("User-agent: *\nContent-Signal: search=yes, ai-input=yes, ai-train=no\nAllow: /\n");
            for agent in TRAINING_CRAWLERS {
                block.push_str(&format!("User-agent: {agent}\nDisallow: /\n"));
            }
            block
        }
    }
}

#[cfg(test)]
#[path = "robots_tests.rs"]
mod tests;
