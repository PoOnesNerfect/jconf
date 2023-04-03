use clap::{Parser, ValueEnum};
use color_eyre::Result;
use glob::{glob, Pattern};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod config_file;
use config_file::get_config_file;

#[derive(Parser, Debug)]
#[command(name = "jconf")]
#[command(author = "PoOnesNerfect <jack.y.l.dev@gmail.com>")]
#[command(version = "0.1.1")]
#[command(about = "Keep all your config files synchronized in one place", long_about = None)]
struct Args {
    #[arg(value_enum, value_name = "COMMAND", default_value_t = Command::Sync)]
    cmd: Command,
    /// File path to jconf config file
    #[arg(short, long, default_value = "./jconf.toml")]
    file: String,
    /// Output path to save the configurations
    #[arg(short, long, value_name = "PATH", default_value = ".")]
    output: String,
    /// Name(s) of the specific config(s) to action on. Ex) `jconf -c helix -c alacritty`
    #[arg(short, long, value_name = "NAME")]
    config: Option<Vec<String>>,
    /// Overwrite the existing files regardless of their modified date
    #[arg(long, default_value_t = false)]
    force: bool,
}

#[derive(Debug, Clone, ValueEnum)]
enum Command {
    /// Synchronize between origin and linked files. Last modified file will overwrite the other.
    Sync,
    /// Pull files from origin paths. Linked files will be overwritten.
    Pull,
    /// Push files to origin paths. Origin files will be overwritten.
    Push,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:?}");
    }
}

fn run() -> Result<()> {
    color_eyre::install()?;

    let Args {
        cmd,
        file,
        config: specific_configs,
        output: output_path,
        force,
    } = Args::parse();
    let output_path = Path::new(&output_path).canonicalize()?;

    // get configs from jconf.toml
    let config_file = get_config_file(&file)?;
    let configs = config_file.reduce_to_configs(specific_configs)?;

    let mut files_changed = false;

    // if configs are specified by arguments, then filter by given configs
    for (config_name, config) in configs {
        let origin_base = config.base_path;
        let linked_base = output_path.join(&config_name);

        let include_glob = config.include_glob;
        let exclude_glob = config.exclude_glob;

        match cmd {
            Command::Pull => {
                let count = sync(
                    &origin_base,
                    &linked_base,
                    &include_glob,
                    &exclude_glob,
                    force,
                )?;

                if count > 0 {
                    files_changed = true;
                    println!("{config_name}: {count} file(s) pulled");
                }
            }
            Command::Push => {
                let count = sync(
                    &linked_base,
                    &origin_base,
                    &include_glob,
                    &exclude_glob,
                    force,
                )?;
                if count > 0 {
                    files_changed = true;
                    println!("{config_name}: {count} file(s) pushed");
                }
            }
            Command::Sync => {
                let pulled = sync(
                    &origin_base,
                    &linked_base,
                    &include_glob,
                    &exclude_glob,
                    false,
                )?;
                let pushed = sync(
                    &linked_base,
                    &origin_base,
                    &include_glob,
                    &exclude_glob,
                    false,
                )?;

                match (pulled, pushed) {
                    (0, 0) => {}
                    (0, pushed) => {
                        files_changed = true;
                        println!("{config_name}: {pushed} file(s) pushed");
                    }
                    (pulled, 0) => {
                        files_changed = true;
                        println!("{config_name}: {pulled} file(s) pulled");
                    }
                    (pulled, pushed) => {
                        files_changed = true;
                        println!(
                            "{config_name}: {pulled} file(s) pulled, and {pushed} file(s) pushed"
                        );
                    }
                }
            }
        }
    }

    if !files_changed {
        println!("No change. All files are up-to-date.")
    }

    Ok(())
}

/// Pull files from origin paths to linked paths.
/// Returns number of files affected.
///
/// This function simply copies over files from origin paths to linked paths.
fn sync(
    from_base: &Path,
    to_base: &Path,
    include_glob: &str,
    exclude_glob: &Option<String>,
    force: bool,
) -> Result<usize> {
    let include_glob = format!(
        "{}/{}",
        from_base.to_string_lossy().trim_end_matches('/'),
        include_glob.trim_start_matches('/')
    );
    let exclude_pat = if let Some(glob_str) = exclude_glob {
        let glob_str = format!(
            "{}/{}",
            from_base.to_string_lossy().trim_end_matches('/'),
            glob_str.trim_start_matches('/')
        );
        Some(Pattern::new(&glob_str)?)
    } else {
        None
    };

    let mut count = 0;

    for entry in glob(&include_glob).expect("failed to read glob pattern") {
        let full_file_path = entry?;

        // if exclude pattern exists, then see if the file matches the glob
        if let Some(exclude_pat) = exclude_pat.as_ref() {
            if exclude_pat.matches_path(&full_file_path) {
                continue;
            }
        }

        let file_path = trim_base_path(&full_file_path, from_base);

        let updated = sync_file(from_base, to_base, &file_path, force)?;
        if updated {
            count += 1;
        }
    }

    Ok(count)
}

fn trim_base_path(full_path: &Path, base_path: &Path) -> PathBuf {
    let mut base_len = base_path.iter().count();
    if base_path.is_file() {
        base_len -= 1;
    }

    full_path.iter().skip(base_len).collect()
}

/// Tries to sync files from from_path to to_path, and returns if file was updated
fn sync_file(from_base: &Path, to_base: &Path, file_path: &Path, force: bool) -> Result<bool> {
    let from_path = from_base.join(file_path);
    let to_path = to_base.join(file_path);

    if from_path.is_dir() || from_path.is_symlink() {
        // we don't process dir since we recursively create all necessary dir when files are synced
        // And I don't want to deal with symlink atm.
        return Ok(false);
    }

    // if to_path already exists, we can skip creating new file and parent directories.
    let to_path_exists = to_path.exists();

    let should_sync = if force || !to_path_exists {
        true
    } else {
        let from_path_mtime = fs::metadata(&from_path)?.modified()?;
        let to_path_mtime = fs::metadata(&to_path)?.modified()?;

        // if origin file was more recently updated than destination file, we should update the destination file
        from_path_mtime > to_path_mtime
    };

    if should_sync {
        // if destination does not exist, create all parent dirs
        if !to_path_exists {
            // recursively create necessary dir
            let mut parent = to_path.to_owned();
            parent.pop();
            fs::create_dir_all(parent)?;
        }

        fs::copy(&from_path, &to_path)?;
    }

    Ok(should_sync)
}
