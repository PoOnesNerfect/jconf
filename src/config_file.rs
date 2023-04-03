use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use serde::{
    de::{self, Deserializer, Visitor},
    Deserialize,
};
use std::{collections::HashMap, env, fs, path::PathBuf};

/// Parse and return data from jconf.toml
pub fn get_config_file(jconf_path: &str) -> Result<ConfigFile> {
    let config_str = fs::read_to_string(jconf_path)
        .with_context(|| format!("Config file {} does not exist", jconf_path))?;

    let jconf = toml::from_str(&config_str)?;

    Ok(jconf)
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigFile {
    pub configs: HashMap<String, ConfigPath>,
}

impl ConfigFile {
    /// Map of config_name -> glob_path_to_config
    pub fn reduce_to_configs(
        mut self,
        specific_configs: Option<Vec<String>>,
    ) -> Result<Vec<(String, ConfigPath)>> {
        let Some(specific_configs) = specific_configs else {
            return Ok(self.configs.into_iter().collect());
        };

        let mut ret = Vec::new();

        for config_name in specific_configs {
            if let Some(glob_pat) = self.configs.remove(&config_name) {
                ret.push((config_name, glob_pat));
            } else {
                return Err(eyre!(
                    "Passed config `{config_name}` does not exist in jconf.toml"
                ));
            }
        }

        Ok(ret)
    }
}

#[derive(Clone, Debug)]
pub struct ConfigPath {
    pub base_path: PathBuf,
    pub include_glob: String,
    pub exclude_glob: Option<String>,
}

pub struct ConfigVisitor;

impl<'de> Visitor<'de> for ConfigVisitor {
    type Value = ConfigPath;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string or a table `{ path, include?, exclude? }`")
    }

    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        let mut include_glob = None;

        Ok(ConfigPath {
            base_path: validated_path(v, &mut include_glob)?,
            include_glob: include_glob.unwrap_or_else(|| "**/*".to_owned()),
            exclude_glob: None,
        })
    }

    fn visit_string<E>(self, v: String) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut base_path = None;
        let mut include_glob = None;
        let mut exclude_glob = None;

        while let Some((k, v)) = map.next_entry::<String, String>()? {
            match k.as_str() {
                "path" => {
                    let path = validated_path(&v, &mut include_glob)?;
                    base_path.replace(path);
                }
                "include" => {
                    // if value is already set from `path` field, don't overwrite it
                    if include_glob.is_none() {
                        include_glob.replace(v);
                    }
                }
                "exclude" => {
                    exclude_glob.replace(v);
                }
                _ => {}
            };
        }

        Ok(ConfigPath {
            base_path: base_path.ok_or_else(|| serde::de::Error::missing_field("path"))?,
            include_glob: include_glob.unwrap_or_else(|| "**/*".to_owned()),
            exclude_glob,
        })
    }
}

// check if given path is a dir with trailing slash.
// If path is a file, remove filename from path and set it as glob.
fn validated_path<E: de::Error>(
    val: &str,
    include_glob: &mut Option<String>,
) -> Result<PathBuf, E> {
    let ends_with_slash = val.ends_with('/');

    let mut path: PathBuf = escape_var(val)
        .map_err(|e| de::Error::custom(format!("{}", e)))?
        .into();

    // if it's a dir but doesn't end with slash,
    // it's invalid
    if !ends_with_slash && path.is_dir() {
        return Err(de::Error::custom(format!(
            "Invalid path: '{val}'. Dir paths must end with a '/'. Ex) `~/.config/helix/`"
        )));
    }

    // if given base_path is a file, then extract the file from base_path,
    // and give it to glob
    if !ends_with_slash {
        let file_name = path
            .file_name()
            .ok_or_else(|| de::Error::custom("Path should not be empty"))?
            .to_str()
            .ok_or_else(|| de::Error::custom("Path should be valid Unicode string"))?
            .to_owned();
        include_glob.replace(file_name);

        path.pop();
    }

    Ok(path)
}

// replaces '~' or env var with its value
fn escape_var(path: &str) -> Result<String> {
    // replace env var with its value
    let path_str = if path.starts_with('$') {
        let path = path.trim_start_matches('$');

        if let Some((env_key, rest)) = path.split_once('/') {
            let mut var = env::var(env_key)?;
            var.push('/');
            var.push_str(rest);
            var
        } else {
            env::var(path)?
        }
    } else {
        path.to_owned()
    };

    let path = if path_str.starts_with('~') {
        let mut home = env::var("HOME").expect("$HOME should exist");

        if let Some((_, rest)) = path_str.split_once('/') {
            home.push('/');
            home.push_str(rest);
            home
        } else {
            home
        }
    } else {
        path_str
    };

    Ok(path)
}

impl<'de> Deserialize<'de> for ConfigPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ConfigVisitor)
    }
}
