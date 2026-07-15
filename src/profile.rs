//! Profiles are DATA, not code. Each file in profiles/ (*.yaml, *.yml, or *.json)
//! deserializes into a `Profile`: how to recognize the device's input nodes, an
//! optional BLE reconnect target, and a `translator` that names one of the
//! built-in translation algorithms plus its parameters/key-map. Onboarding a
//! device that fits an existing translator needs no recompile — just a new file.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use evdev::InputEvent;
use serde::Deserialize;

use crate::gesture::{name as gesture_name, GestureEngine};
use crate::keys;

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    pub reconnect: Option<Reconnect>,
    pub translator: TranslatorSpec,
}

#[derive(Debug, Deserialize)]
pub struct MatchSpec {
    pub name_exact: Option<String>,
    pub name_prefix: Option<String>,
    pub name_contains: Option<String>,
}

impl MatchSpec {
    pub fn matches(&self, node_name: &str) -> bool {
        if let Some(s) = &self.name_exact {
            return node_name == s;
        }
        if let Some(s) = &self.name_prefix {
            return node_name.starts_with(s);
        }
        if let Some(s) = &self.name_contains {
            return node_name.contains(s);
        }
        false
    }
}

#[derive(Debug, Deserialize)]
pub struct Reconnect {
    pub mac: String,
    pub adapter: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranslatorSpec {
    /// Device fakes a touchpad; classify swipes into up/down/left/right/tap.
    Gesture {
        tap_dist: Option<f64>,
        map: HashMap<String, String>,
    },
    /// Device sends real key codes; remap input key name -> output key name.
    Keymap { map: HashMap<String, String> },
}

impl TranslatorSpec {
    pub fn build(&self) -> Result<Translator> {
        match self {
            TranslatorSpec::Gesture { tap_dist, map } => {
                let mut out = HashMap::new();
                for (g, keyname) in map {
                    let code = keys::code(keyname)
                        .ok_or_else(|| anyhow!("unknown key name '{keyname}'"))?;
                    out.insert(g.to_lowercase(), code);
                }
                Ok(Translator::Gesture {
                    engine: GestureEngine::new(tap_dist.unwrap_or(400.0)),
                    map: out,
                })
            }
            TranslatorSpec::Keymap { map } => {
                let mut out = HashMap::new();
                for (from, to) in map {
                    let fc = keys::code(from)
                        .ok_or_else(|| anyhow!("unknown input key name '{from}'"))?;
                    let tc =
                        keys::code(to).ok_or_else(|| anyhow!("unknown output key name '{to}'"))?;
                    out.insert(fc, tc);
                }
                Ok(Translator::Keymap { map: out })
            }
        }
    }
}

/// Runtime translator (holds per-device state). `handle` returns the output key
/// code to inject, if any.
pub enum Translator {
    Gesture {
        engine: GestureEngine,
        map: HashMap<String, u16>,
    },
    Keymap {
        map: HashMap<u16, u16>,
    },
}

impl Translator {
    pub fn handle(&mut self, ev: &InputEvent) -> Option<u16> {
        match self {
            Translator::Gesture { engine, map } => {
                let g = engine.feed(ev)?;
                map.get(gesture_name(g)).copied()
            }
            Translator::Keymap { map } => {
                if ev.event_type() == evdev::EventType::KEY && ev.value() == 1 {
                    map.get(&ev.code()).copied()
                } else {
                    None
                }
            }
        }
    }
}

/// Load every profile file from a directory.
pub fn load_dir(dir: &Path) -> Result<Vec<Profile>> {
    let mut profiles = Vec::new();
    let entries =
        fs::read_dir(dir).with_context(|| format!("reading profiles dir {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let text = match ext {
            "yaml" | "yml" | "json" => {
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
            }
            _ => continue,
        };
        let profile: Profile = if ext == "json" {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        };
        // fail fast on bad key names / translator config
        profile
            .translator
            .build()
            .with_context(|| format!("in profile {}", path.display()))?;
        profiles.push(profile);
    }
    if profiles.is_empty() {
        bail!("no profiles found in {}", dir.display());
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gesture_profile_and_matches() {
        let yaml = r#"
name: t
match:
  name_prefix: "JX"
translator:
  kind: gesture
  map:
    up: KEY_UP
    tap: KEY_ENTER
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.name, "t");
        assert!(p.match_spec.matches("JX-05"));
        assert!(!p.match_spec.matches("Other"));
        p.translator.build().unwrap();
    }

    #[test]
    fn unknown_key_name_fails_to_build() {
        let yaml = r#"
name: t
match:
  name_exact: "X"
translator:
  kind: keymap
  map:
    KEY_A: KEY_BOGUS
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(p.translator.build().is_err());
    }

    #[test]
    fn json_profile_also_parses() {
        let json = r#"{"name":"j","match":{"name_contains":"Ring"},
            "translator":{"kind":"keymap","map":{"KEY_VOLUMEUP":"KEY_UP"}}}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(p.match_spec.matches("My Ring"));
        p.translator.build().unwrap();
    }
}
