//! Request forms: the `:::form` markup, the site's public description of its
//! forms (`_moss/commerce.json`), and the checks that need the whole site.
//!
//! The markup is a plain HTML form that works with no script and posts to
//! `<endpoint>/<form name>`; `forms.js` upgrades it to a post that stays on
//! the page. Wire contract: the body is `application/x-www-form-urlencoded`,
//! `_next` is where the service sends the reader, and `_hp` is a honeypot.

use super::{html_escape, CHECK_SVG};
use crate::i18n::{t, Language};
use moss_core::ast::{FieldKind, FormField, FormShortcode};

fn input_type(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Email => "email",
        FieldKind::Tel => "tel",
        FieldKind::Date => "date",
        _ => "text",
    }
}

/// `aria-describedby` for a control: its help text, when it has any.
fn described_by(help_id: &Option<String>) -> String {
    help_id.as_ref().map(|id| format!(r#" aria-describedby="{id}""#)).unwrap_or_default()
}

fn render_field(form: &str, f: &FormField, lang: Language) -> String {
    let id = format!("{form}-{}", f.name);
    let help_id = f.help.as_ref().map(|_| format!("{id}-help"));
    let name = html_escape(&f.name);
    let label = html_escape(&f.label);
    let optional = if f.required {
        String::new()
    } else {
        format!(r#" <span class="moss-form__optional">{}</span>"#, t(lang, "form_optional"))
    };
    let help = f
        .help
        .as_ref()
        .zip(help_id.as_ref())
        .map(|(h, hid)| {
            let text = h.split("\n\n").map(html_escape).collect::<Vec<_>>().join("<br><br>");
            format!("\n    <p class=\"moss-form__help\" id=\"{hid}\">{text}</p>")
        })
        .unwrap_or_default();
    let error = format!("\n    <p class=\"moss-form__error\" id=\"{id}-error\" hidden></p>");
    let required = if f.required { " required" } else { "" };
    let auto = f.autocomplete.as_ref().map(|a| format!(r#" autocomplete="{}""#, html_escape(a))).unwrap_or_default();
    let describe = described_by(&help_id);
    let attr = format!(r#" id="{id}" name="{name}"{required}{auto}{describe}"#);
    let choice_inputs = |kind: &str, req: &str| {
        f.choices
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let v = html_escape(c);
                format!(
                    r#"      <div class="moss-form__choice"><input type="{kind}" id="{id}-{i}" name="{name}" value="{v}"{req}><label for="{id}-{i}">{v}</label></div>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    match f.kind {
        FieldKind::Radio | FieldKind::Checkbox if !f.choices.is_empty() => {
            let kind = if f.kind == FieldKind::Radio { "radio" } else { "checkbox" };
            let req = if f.kind == FieldKind::Radio { required } else { "" };
            format!(
                "  <fieldset class=\"moss-form__field moss-form__group\" data-field=\"{name}\"{describe}>\n    <legend>{label}{optional}</legend>\n{}{help}{error}\n  </fieldset>",
                choice_inputs(kind, req)
            )
        }
        FieldKind::Checkbox => format!(
            "  <div class=\"moss-form__field\" data-field=\"{name}\">\n    <div class=\"moss-form__choice\"><input type=\"checkbox\" value=\"on\"{attr}><label for=\"{id}\">{label}{optional}</label></div>{help}{error}\n  </div>"
        ),
        kind => {
            let control = match kind {
                FieldKind::Textarea => format!(r#"<textarea class="moss-input moss-input--boxed" rows="5"{attr}></textarea>"#),
                FieldKind::Select => {
                    let options = f
                        .choices
                        .iter()
                        .map(|c| format!(r#"<option value="{v}">{v}</option>"#, v = html_escape(c)))
                        .collect::<String>();
                    format!(r#"<select class="moss-input"{attr}><option value=""></option>{options}</select>"#)
                }
                other => format!(r#"<input class="moss-input" type="{}"{attr}>"#, input_type(other)),
            };
            format!(
                "  <div class=\"moss-form__field\" data-field=\"{name}\">\n    <label class=\"moss-form__label\" for=\"{id}\">{label}{optional}</label>\n    {control}{help}{error}\n  </div>"
            )
        }
    }
}

/// The form's HTML. With no `endpoint` the form is marked pending: a published
/// page hides it, and moss's preview shows it with sending simulated.
pub fn render_form(form: &FormShortcode, lang: Language, endpoint: Option<&str>) -> String {
    let Some(name) = form.name.as_deref() else { return String::new() };
    let (action, pending) = super::hosted_action(endpoint.map(|e| format!("{e}/{name}")));
    let next = html_escape(form.next.as_deref().unwrap_or(""));
    let label = form.button.as_deref().filter(|b| !b.is_empty()).map(html_escape).unwrap_or_else(|| t(lang, "form_send").to_string());
    let fields = form.fields.iter().map(|f| render_field(name, f, lang)).collect::<Vec<_>>().join("\n");
    let copy = [
        ("required", "form_required"),
        ("format", "form_format"),
        ("invalid", "form_invalid"),
        ("rate_limited", "form_rate_limited"),
        ("unknown_form", "form_unknown"),
        ("network", "form_network"),
    ]
    .iter()
    .map(|(k, key)| format!(r#"<span data-copy="{k}">{}</span>"#, t(lang, key)))
    .collect::<String>();
    format!(
        r#"<div class="moss-form" data-state="idle"{pending}>
<form method="post" action="{action}" data-moss-form="{name}" data-next="{next}">
{fields}
  <input type="hidden" name="_next" value="{next}">
  <div class="moss-form__hp" aria-hidden="true"><label for="{name}-hp">{hp}</label><input type="text" id="{name}-hp" name="_hp" value="" tabindex="-1" autocomplete="off"></div>
  <span class="moss-btn-slot">
    <button type="submit" class="moss-btn">
      <span class="moss-btn__label">{label}</span>
      <span class="moss-btn__spinner" aria-hidden="true"></span>
      <span class="moss-btn__check" aria-hidden="true">{CHECK_SVG}</span>
    </button>
  </span>
  <p class="moss-form__status" aria-live="polite"></p>
  <div hidden>{copy}</div>
</form>
</div>"#,
        action = html_escape(&action),
        hp = t(lang, "form_hp_label"),
    )
}

#[cfg(test)]
#[path = "forms_tests.rs"]
mod tests;
