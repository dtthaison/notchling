//! Reading the Swift app's settings, read-only, and moving them into the app's own `settings.json`.
//!
//! The Swift app keeps its settings in the UserDefaults domain `local.notchling` (and, from before
//! it was renamed, `local.awake.menubar`). The engine's migration ([`notchling_engine::settings::
//! import`]) turns an exported domain into new settings; reading the domain is the app's job, and
//! this is it. The source is a trait: the real one runs `defaults export <domain> -`, which prints
//! the domain as a property list and changes nothing (the cfprefsd cache is current, which a read
//! of the plist file is not). It is never given another verb, so the Swift app's preferences are
//! never written. Tests read a fixture plist through [`PlistFiles`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use notchling_engine::control::{Command, Launcher};
use notchling_engine::settings::{import, DefaultsExport, Report, Settings};
use serde_json::{Map, Value};

use super::xml::{self, Node};

/// The Swift app's domain.
pub const DOMAIN: &str = "local.notchling";
/// The domain before it, which the Swift app itself migrates from once.
pub const AWAKE_DOMAIN: &str = "local.awake.menubar";

/// Where a domain's property list comes from.
pub trait DefaultsSource: Send + Sync {
    /// The domain as XML property list text, or `None` when it has no preferences.
    fn export(&self, domain: &str) -> Result<Option<String>, String>;
}

/// The real source: `/usr/bin/defaults export <domain> -`. The only verb it knows is `export`.
pub struct DefaultsTool<L: Launcher> {
    launcher: L,
}

impl<L: Launcher> DefaultsTool<L> {
    pub fn new(launcher: L) -> Self {
        DefaultsTool { launcher }
    }

    /// The command for `domain`; public so a test can pin it.
    pub fn command(domain: &str) -> Command {
        Command::new(&["/usr/bin/defaults", "export", domain, "-"])
    }
}

impl<L: Launcher> DefaultsSource for DefaultsTool<L> {
    fn export(&self, domain: &str) -> Result<Option<String>, String> {
        let out = self
            .launcher
            .run(&Self::command(domain), Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        if out.code != 0 {
            let said = out.combined();
            return if said.contains("does not exist") {
                Ok(None)
            } else {
                Err("defaults export failed".into())
            };
        }
        Ok(Some(out.output).filter(|text| !text.trim().is_empty()))
    }
}

/// Property list files in a folder, one `<domain>.plist` each: a fixture folder in tests.
pub struct PlistFiles {
    pub dir: PathBuf,
}

impl DefaultsSource for PlistFiles {
    fn export(&self, domain: &str) -> Result<Option<String>, String> {
        let path = self.dir.join(format!("{domain}.plist"));
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// No preferences at all: a render's, and a fixture root's.
pub struct NoDefaults;

impl DefaultsSource for NoDefaults {
    fn export(&self, _domain: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

/// An XML property list's top dictionary as JSON. Strings, numbers and booleans are themselves;
/// `<data>` is its base64 text and `<date>` its ISO text, which is how the engine's migration
/// expects an export (it decodes the data keys that hold JSON).
pub fn parse_plist(text: &str) -> Result<Map<String, Value>, String> {
    let root = xml::parse(text)?;
    if root.name != "plist" {
        return Err("not a property list".into());
    }
    let top = root.children.first().ok_or("an empty property list")?;
    match value(top)? {
        Value::Object(map) => Ok(map),
        _ => Err("the property list isn't a dictionary".into()),
    }
}

fn value(node: &Node) -> Result<Value, String> {
    Ok(match node.name.as_str() {
        "string" | "date" => Value::String(node.text.clone()),
        "data" => Value::String(node.text.split_whitespace().collect()),
        "integer" => node
            .text
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "a bad integer")?,
        "real" => node
            .text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or("a bad real")?,
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "array" => Value::Array(node.children.iter().map(value).collect::<Result<_, _>>()?),
        "dict" => {
            let mut map = Map::new();
            let mut pairs = node.children.chunks(2);
            for pair in &mut pairs {
                let [key, item] = pair else {
                    return Err("a dictionary key with no value".into());
                };
                if key.name != "key" {
                    return Err("a dictionary entry with no key".into());
                }
                map.insert(key.text.clone(), value(item)?);
            }
            Value::Object(map)
        }
        other => return Err(format!("an unknown property list type <{other}>")),
    })
}

/// Both domains, read.
pub fn read_export(source: &dyn DefaultsSource) -> Result<DefaultsExport, String> {
    let current = match source.export(DOMAIN)? {
        Some(text) => parse_plist(&text)?,
        None => Map::new(),
    };
    let awake = match source.export(AWAKE_DOMAIN)? {
        Some(text) => Some(parse_plist(&text)?),
        None => None,
    };
    Ok(DefaultsExport { current, awake })
}

/// What an import found and did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    /// Settings now in the app's file that the Swift domain supplied (not already there).
    pub added: Vec<String>,
    /// Settings the app already had, which win and were left alone.
    pub kept: Vec<String>,
    /// Keys the export had that aren't settings (window frames and the like).
    pub ignored: usize,
    /// Settings whose value was the wrong type; dropped.
    pub invalid: Vec<String>,
}

impl Imported {
    /// The line the first-run view shows.
    pub fn line(&self) -> String {
        match (self.added.len(), self.kept.len()) {
            (0, 0) => "The Swift app has no settings to bring over.".into(),
            (0, _) => "Nothing new: every setting is already here.".into(),
            (n, _) => format!("Brought over {n} setting{}.", if n == 1 { "" } else { "s" }),
        }
    }
}

/// Reads the Swift app's settings and puts them into `file`, the app's own. Values already in
/// `file` win, so running it again, or after a setting was changed here, loses nothing. Only
/// `file` is written; an unreadable `file` is left as it is.
pub fn import_into(source: &dyn DefaultsSource, file: &Path) -> Result<Imported, String> {
    let export = read_export(source)?;
    let mut settings = Settings::open(file).map_err(|e| e.to_string())?;
    let before: Map<String, Value> = settings.values().clone();
    let preview = notchling_engine::settings::migrate(&export);
    let report: Report = import(&mut settings, &export).map_err(|e| e.to_string())?;
    let mut done = Imported {
        ignored: report.ignored.len(),
        invalid: report.invalid,
        ..Imported::default()
    };
    for name in preview.values.keys() {
        if before.contains_key(name) {
            done.kept.push(name.clone());
        } else {
            done.added.push(name.clone());
        }
    }
    done.added.sort();
    done.kept.sort();
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::control::{FakeLauncher, LaunchOutput};
    use serde_json::json;

    fn fixtures() -> PlistFiles {
        PlistFiles {
            dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/welcome"),
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-welcome-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plist_becomes_json() {
        let map = parse_plist(
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>a</key><true/><key>b</key><integer>-4</integer><key>c</key><real>1.5</real>
            <key>d</key><array><string>x</string><false/></array>
            <key>e</key><data>AAEC
            AwQ=</data><key>f</key><dict/></dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            Value::Object(map),
            json!({"a": true, "b": -4, "c": 1.5, "d": ["x", false], "e": "AAECAwQ=", "f": {}})
        );
        for bad in [
            "<plist><array/></plist>",
            "<dict/>",
            "<plist><dict><key>a</key></dict></plist>",
            "<plist><dict><key>a</key><weird/></dict></plist>",
            "<plist><dict><key>a</key><integer>x</integer></dict></plist>",
            "<plist></plist>",
        ] {
            assert!(parse_plist(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_fixture_domain_is_read_with_its_awake_domain() {
        let export = read_export(&fixtures()).unwrap();
        assert_eq!(export.current["mode"], json!("whileWorking"));
        assert_eq!(export.current["hideNotch"], json!(1));
        assert_eq!(
            export.current["chatProjectFolders"],
            json!(["/work/notchling", "/work/api & tools"])
        );
        let awake = export.awake.unwrap();
        assert_eq!(awake["active"], json!(true));
    }

    #[test]
    fn import_brings_settings_over_and_is_idempotent() {
        let dir = temp("import");
        let file = dir.join("settings.json");
        let first = import_into(&fixtures(), &file).unwrap();
        assert!(first.added.contains(&"mode".to_string()));
        assert!(first.added.contains(&"accounts".to_string()));
        assert!(first.kept.is_empty());
        // The window frame is not a setting; the date under a bool key is the wrong type.
        assert_eq!(first.ignored, 1);
        assert_eq!(first.invalid, vec!["askedAccessibility".to_string()]);
        assert_eq!(
            first.line(),
            format!("Brought over {} settings.", first.added.len())
        );

        let store = Settings::open(&file).unwrap();
        assert_eq!(store.get_str("mode").as_deref(), Some("whileWorking"));
        assert_eq!(store.get_bool("hideNotch"), Some(true), "1 reads as a bool");
        assert_eq!(store.get_bool("showPet"), Some(false));
        assert_eq!(store.get_f64("chatPageWidth"), Some(480.5));
        // The saved accounts were data holding JSON: they arrive as that JSON.
        assert_eq!(
            store.raw("accounts"),
            Some(&json!([{"email": "me@example.com", "name": "Me"}]))
        );
        // limitSwitchAt is kept as written and held to 80 to 99 on read.
        assert_eq!(store.get_i64("limitSwitchAt"), Some(99));

        // Changed here since, then imported again: the change wins and nothing else moves.
        let mut store = store;
        store.set("mode", json!("off")).unwrap();
        let before = std::fs::read_to_string(&file).unwrap();
        let again = import_into(&fixtures(), &file).unwrap();
        assert!(again.added.is_empty() && again.kept.len() == first.added.len());
        assert_eq!(again.line(), "Nothing new: every setting is already here.");
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(after, serde_json::from_str::<Value>(&before).unwrap());
        assert_eq!(after["values"]["mode"], json!("off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_apps_own_choices_survive_an_import() {
        let dir = temp("own");
        let file = dir.join("settings.json");
        let mut store = Settings::open(&file).unwrap();
        let mut values = store.values().clone();
        values.insert("actingOn".into(), json!(true));
        store.replace_all(values).unwrap();
        import_into(&fixtures(), &file).unwrap();
        assert_eq!(
            Settings::open(&file).unwrap().raw("actingOn"),
            Some(&json!(true))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_domain_means_nothing_to_bring_over() {
        let dir = temp("none");
        let file = dir.join("settings.json");
        let done = import_into(&NoDefaults, &file).unwrap();
        assert_eq!(done.line(), "The Swift app has no settings to bring over.");
        let empty = import_into(&PlistFiles { dir: dir.clone() }, &file).unwrap();
        assert_eq!(empty, done);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_settings_file_is_left_alone() {
        let dir = temp("broken");
        let file = dir.join("settings.json");
        std::fs::write(&file, "{not json").unwrap();
        assert!(import_into(&fixtures(), &file).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_real_source_only_ever_exports() {
        let launcher = FakeLauncher::new();
        launcher.push_output(Ok(LaunchOutput::ok("<plist><dict/></plist>")));
        launcher.push_output(Ok(LaunchOutput::failed(1, "")));
        let tool = DefaultsTool::new(launcher);
        assert!(tool.export(DOMAIN).unwrap().is_some());
        assert!(tool.export(AWAKE_DOMAIN).is_err());
        let seen = tool.launcher.commands();
        assert_eq!(
            seen[0].argv,
            ["/usr/bin/defaults", "export", "local.notchling", "-"]
        );
        for command in &seen {
            assert_eq!(command.argv[1], "export", "never write or delete");
        }
    }

    #[test]
    fn a_missing_domain_is_not_an_error() {
        let launcher = FakeLauncher::new();
        launcher.push_output(Ok(LaunchOutput {
            code: 1,
            output: String::new(),
            stderr: "Domain local.awake.menubar does not exist".into(),
        }));
        launcher.push_output(Ok(LaunchOutput::ok("  \n")));
        let tool = DefaultsTool::new(launcher);
        assert_eq!(tool.export(AWAKE_DOMAIN), Ok(None));
        assert_eq!(tool.export(DOMAIN), Ok(None));
    }
}
