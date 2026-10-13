use super::*;
use crate::plugins::types::PluginManifest;

fn plugin(name: &str, derivations: &[(&str, &[&str])]) -> Plugin {
    let body: Vec<String> = derivations
        .iter()
        .map(|(id, accepts)| {
            format!(
                r##""{id}":{{"accepts":{},"noun":{{"one":"doc","other":"docs"}},
                    "fragments":{{"text":"markdown"}},"stub":["#text"],
                    "run":{{"binary":"unpack","artifact":[{{"kind":"path","binary":"/bin/true"}}]}}}}"##,
                serde_json::to_string(accepts).unwrap()
            )
        })
        .collect();
    let json = format!(
        r#"{{"name":"{name}","version":"2.0.0","contributes":{{"derivations":{{{}}}}}}}"#,
        body.join(",")
    );
    Plugin {
        manifest: PluginManifest::parse(&json).unwrap(),
        path: PathBuf::from(format!("/plugins/{name}")),
    }
}

#[test]
fn an_extension_resolves_to_its_plugin_in_any_spelling() {
    let reg = DerivationRegistry::from_plugins(&[plugin("docs", &[("pages", &["xyz", "abc"])])]);
    for spelling in ["xyz", ".xyz", "XYZ"] {
        let e = reg.for_extension(spelling).expect(spelling);
        assert_eq!((e.plugin_id.as_str(), e.derivation_id.as_str()), ("docs", "pages"));
        assert_eq!(e.plugin_version, "2.0.0");
        assert_eq!(e.plugin_dir, PathBuf::from("/plugins/docs"));
    }
    assert!(reg.for_extension("md").is_none());
    assert!(reg.diagnostics().is_empty());
}

#[test]
fn two_plugins_on_one_extension_resolve_by_name_not_by_discovery_order() {
    let a = plugin("alpha", &[("pages", &["xyz"])]);
    let b = plugin("beta", &[("pages", &["xyz"])]);
    for order in [vec![a.clone(), b.clone()], vec![b, a]] {
        let reg = DerivationRegistry::from_plugins(&order);
        assert_eq!(reg.for_extension("xyz").unwrap().plugin_id, "alpha");
        assert_eq!(reg.diagnostics().len(), 1);
        assert!(reg.diagnostics()[0].contains("beta/pages"), "{:?}", reg.diagnostics());
    }
}

#[test]
fn a_plugin_without_derivations_contributes_nothing() {
    let p = Plugin {
        manifest: PluginManifest::parse(r#"{"name":"x","version":"1.0.0","entry":"main.js"}"#).unwrap(),
        path: PathBuf::new(),
    };
    assert!(DerivationRegistry::from_plugins(&[p]).is_empty());
}
