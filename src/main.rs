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
#[command(version = "0.1")]
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

    // if configs are specified by arguments, then filter by given configs
    for (config_name, config) in configs {
        let origin_base = config.base_path;
        let linked_base = output_path.join(config_name);

        let include_glob = config.include_glob;
        let exclude_glob = config.exclude_glob;

        match cmd {
            Command::Pull => sync(
                &origin_base,
                &linked_base,
                &include_glob,
                &exclude_glob,
                force,
            )?,
            Command::Push => sync(
                &linked_base,
                &origin_base,
                &include_glob,
                &exclude_glob,
                force,
            )?,
            Command::Sync => {
                sync(
                    &origin_base,
                    &linked_base,
                    &include_glob,
                    &exclude_glob,
                    false,
                )?;
                sync(
                    &linked_base,
                    &origin_base,
                    &include_glob,
                    &exclude_glob,
                    false,
                )?;
            }
        };
    }

    Ok(())
}

/// Pull files from origin paths to linked paths.
///
/// This function simply copies over files from origin paths to linked paths.
fn sync(
    from_base: &Path,
    to_base: &Path,
    include_glob: &str,
    exclude_glob: &Option<String>,
    force: bool,
) -> Result<()> {
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

    for entry in glob(&include_glob).expect("failed to read glob pattern") {
        let full_file_path = entry?;

        // if exclude pattern exists, then see if the file matches the glob
        if let Some(exclude_pat) = exclude_pat.as_ref() {
            if exclude_pat.matches_path(&full_file_path) {
                continue;
            }
        }

        let file_path = trim_base_path(&full_file_path, from_base);

        sync_file(from_base, to_base, &file_path, force)?;
    }

    Ok(())
}

fn trim_base_path(full_path: &Path, base_path: &Path) -> PathBuf {
    let mut base_len = base_path.iter().count();
    if base_path.is_file() {
        base_len -= 1;
    }

    full_path.iter().skip(base_len).collect()
}

fn sync_file(from_base: &Path, to_base: &Path, file_path: &Path, force: bool) -> Result<()> {
    let from_path = from_base.join(file_path);
    let to_path = to_base.join(file_path);

    if from_path.is_dir() {
        // we don't process dir since we recursively create all necessary dir when files are synced
        return Ok(());
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
        // I don't wanna deal with links for now
        if from_path.is_file() {
            // if destination does not exist, create all parent dirs
            if !to_path_exists {
                // recursively create necessary dir
                let mut parent = to_path.to_owned();
                parent.pop();
                fs::create_dir_all(parent)?;
            }

            fs::copy(&from_path, &to_path)?;
        }
    }

    Ok(())
}
