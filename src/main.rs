mod bisecting;
mod release_hub;

use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;

use bisecting::round_date;
use release_hub::{GitTag, GithubRelease, ReleaseAsset, ReleaseHub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Goodness {
    Good,
    Bad,
    Skip,
    // Unknown,
}

#[derive(serde::Deserialize, Debug)]
struct Config {
    distr_dir: std::path::PathBuf,
    unpack_dir: std::path::PathBuf,
    userdata_dir: std::path::PathBuf,
    zip_extractor_path: std::path::PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReleaseBlacklist {
    release_tags: std::collections::BTreeSet<String>,
}
impl ReleaseBlacklist {
    fn blacklist_file() -> std::path::PathBuf {
        std::path::Path::new("cache/blacklist.json").into()
    }
    fn load() -> anyhow::Result<Self> {
        let file = &Self::blacklist_file();
        if !file.exists() {
            let out = Self {
                release_tags: Default::default(),
            };
            out.save().with_context(|| format!("saving blacklist?"))?;
            return Ok(out);
        }
        
        let out = serde_json::from_str(
            &std::fs::read_to_string(file).with_context(|| format!("reading {:?}", file))?,
        )?;
        Ok(out)
    }
    fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(Self::blacklist_file().parent().unwrap()).ok();
        Ok(std::fs::File::create(Self::blacklist_file())?
            .write_all(serde_json::to_string_pretty(self)?.as_bytes())?)
    }
    fn add(&mut self, release: &GithubRelease) -> anyhow::Result<()> {
        self.release_tags.insert(release.tag_name.to_string());
        self.save()
    }
}

fn asset_unpack_dir(cfg: &Config, asset: &ReleaseAsset) -> PathBuf {
    cfg.unpack_dir.join(asset.name.split(".").next().unwrap())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Track(Vec<(String, Goodness)>);
impl Track {
    fn new() -> Self {
        Track(vec![])
    }
    fn load() -> anyhow::Result<Self> {
        let track_f = std::path::Path::new("cache/track.json");
        if track_f.exists() {
            return Ok(serde_json::from_str(&std::fs::read_to_string(track_f)?)?);
        };
        Ok(Track::new())
    }
    fn save(&self) -> anyhow::Result<()> {
        std::fs::File::create("cache/track.json")?
            .write_all(serde_json::to_string_pretty(&self)?.as_bytes())?;
        Ok(())
    }
    fn is_tag_skipped(&self, tag: &GitTag) -> bool {
        let marked = self.0.iter().find(|t| t.0 == tag.name);
        let Some(marked) = marked else {
            return false;
        };
        return marked.1 == Goodness::Skip;
    }
}

fn select_best_asset(release: &GithubRelease) -> &ReleaseAsset {
    let prio_list = vec![
        "cdda-windows-tiles-x64-msvc",
        "cdda-windows-with-graphics-x64",
        "cdda-windows-tiles-x64",
        "cdda-windows-tiles",
        "cdda-windows-with-graphics",
    ];
    for prio in prio_list {
        let candidates = release
            .assets
            .iter()
            .filter(|a| a.name.starts_with(prio))
            .collect::<Vec<_>>();
        if let Some(a) = candidates.first() {
            return *a;
        }
    }
    panic!("this should never happen (release: {:?})", release);
}
use bisecting::Roundness;
fn select_earlier_release<'a>(
    releases: &'a ReleaseHub,
    rough_date: Option<chrono::NaiveDate>,
) -> anyhow::Result<&'a GitTag> {
    // let earliest_release = releases.find_tag(earliest_tag);
    // let earliest_date = earliest_release.tag_datetime().date();

    // let earlier = earliest_date - chrono::Days::new(ddays);
    let earlier = match rough_date {
        Some(d) => d,
        None => releases.tags_list.first().unwrap().tag_datetime().date(),
    };

    let now = chrono::Utc::now();
    let days_since = (now.date_naive() - earlier).num_days();

    let roundness = match days_since {
        x if x < 3 => Roundness::Day,
        x if x < 14 => Roundness::Week,
        _ => Roundness::Month,
    };

    let earlier = round_date(earlier, roundness);

    println!(
        "Rought date {:?} rounded to nearest {:?} to {:?} ",
        rough_date, roundness, earlier
    );

    // loop {
    //     let have_candidate = releases
    //         .tags_list
    //         .iter()
    //         .any(|r| r.tag_datetime().date() <= earlier);
    //     if have_candidate {
    //         break;
    //     } else {
    //         releases.fetch_more_releases()?;
    //     }
    // }
    let earlier_release = releases
        .tags_list
        .iter()
        .filter(|r| r.tag_datetime().date() == earlier)
        .last()
        .with_context(|| anyhow::format_err!("no releases match the date of {:?}", earlier))?;
    Ok(earlier_release)
    // let earlier_release = earlier_release.unwrap();
    // Ok(earlier_release)
}

struct BisectState {
    config: Config,
    releases: ReleaseHub,
    active_install: Option<(GithubRelease, ReleaseAsset)>,
    track: Track,
}
impl BisectState {
    fn new() -> anyhow::Result<Self> {
        let config: Config = json5::from_str(&std::fs::read_to_string("config.json5")?)?;
        let releases = ReleaseHub::load().with_context(|| format!("grabbing releases"))?;
        let track = Track::load()?;
        let mut out = Self {
            config,
            releases,
            active_install: None,
            track: Track::load()?,
        };
        if let Some((v, _)) = track.0.last().cloned() {
            out.activate_tag(&v)?;
        };
        Ok(out)
    }
    fn fetch_more_releases(&mut self) -> anyhow::Result<()> {
        self.releases.fetch_more_releases()
    }
    fn activate_release(&mut self, release: &GithubRelease) -> anyhow::Result<()> {
        // let active_version = &self.releases[0].clone();
        let asset = select_best_asset(release);
        self.activate_asset(asset)?;
        self.active_install = Some((release.clone(), asset.clone()));
        Ok(())
    }
    fn activate_asset(&mut self, asset: &ReleaseAsset) -> anyhow::Result<()> {
        log::info!("Activating version {:?}", asset.name);
        std::fs::create_dir_all(&self.config.distr_dir)?;
        let distr_file = &self.config.distr_dir.join(&asset.name);
        if !distr_file.exists() {
            log::info!(
                "Downloading {} -> {}..",
                asset.browser_download_url,
                distr_file.to_string_lossy()
            );
            let mut buf = vec![];
            ureq::get(&asset.browser_download_url)
                .call()?
                .into_reader()
                .read_to_end(&mut buf)?;
            std::fs::File::create(distr_file)?.write_all(&buf)?;
            log::info!("..done");
        }

        let unpacked_dir = self
            .config
            .unpack_dir
            .join(asset.name.split(".").next().unwrap());
        anyhow::ensure!(asset.name.ends_with(".zip"));
        if !unpacked_dir.exists() {
            log::info!(
                "Unpacking {} -> {} ... ",
                distr_file.to_string_lossy(),
                unpacked_dir.to_string_lossy()
            );
            let unpacked_dir_tmp = &self.config.unpack_dir.join("_unpack_tmp");
            if unpacked_dir_tmp.exists() {
                std::fs::remove_dir_all(unpacked_dir_tmp)?;
            }
            // assuming 7-zip
            std::process::Command::new(&self.config.zip_extractor_path)
                .args(["x", "-aou"]) // extract, overwrite always
                .args(["-bb0"])
                // .args(["-bd"]) // disable output progress
                .arg(format!("-o{}", unpacked_dir_tmp.to_string_lossy())) //output dir
                .arg(distr_file)
                .status()?;
            std::fs::rename(unpacked_dir_tmp, unpacked_dir)?;
            log::info!("..done");
        }
        Ok(())
    }

    fn launch(&self) -> anyhow::Result<()> {
        let active_dir = asset_unpack_dir(
            &self.config,
            &self
                .active_install
                .as_ref()
                .with_context(|| anyhow::format_err!("no active install"))?
                .1,
        );
        let game_dir = &active_dir;
        let userdata_dir = &self.config.userdata_dir;
        if !userdata_dir.exists() {
            std::fs::create_dir_all(userdata_dir)?;
        };
        let mut proc = std::process::Command::new(game_dir.join("cataclysm-tiles.exe"))
            .args(["--basepath", &game_dir.to_string_lossy()])
            .args(["--userdir", &userdata_dir.to_string_lossy()])
            .spawn()
            .with_context(|| {
                anyhow::format_err!("game dir: {}", game_dir.as_os_str().to_string_lossy())
            })?;
        proc.wait()?;
        Ok(())
    }

    fn mark_good(&mut self) -> anyhow::Result<()> {
        let (release, _asset) = self.active_install.as_ref().unwrap();
        self.track
            .0
            .push((release.tag_name.clone(), Goodness::Good));
        self.track.save()?;
        Ok(())
    }
    fn mark_bad(&mut self) -> anyhow::Result<()> {
        let (release, _asset) = self.active_install.as_ref().unwrap();
        self.track.0.push((release.tag_name.clone(), Goodness::Bad));
        self.track.save()?;
        Ok(())
    }
    fn mark_skip(&mut self) -> anyhow::Result<()> {
        let (release, _asset) = self.active_install.as_ref().unwrap();
        self.track
            .0
            .push((release.tag_name.clone(), Goodness::Skip));
        self.track.save()?;
        Ok(())
    }
    fn mark_blacklist(&mut self) -> anyhow::Result<()> {
        let (release, _asset) = self.active_install.clone().unwrap();
        self.mark_skip()?;
        self.releases.mark_blacklist(&release)?;
        Ok(())
    }
    fn show_track(&self) -> anyhow::Result<()> {
        for (tag, good) in &self.track.0 {
            println!("{} - {:?}", tag, good);
        }
        Ok(())
    }
    fn advance(&mut self, args: Option<&str>) -> anyhow::Result<()> {
        let latest_good = self
            .track
            .0
            .iter()
            .filter(|(_, g)| g == &Goodness::Good)
            .map(|(r, _)| r)
            .max();
        let earliest_bad = self
            .track
            .0
            .iter()
            .filter(|(_, g)| g == &Goodness::Bad)
            .map(|(r, _)| r)
            .min();
        if earliest_bad.is_none() {
            log::info!("No bad versions recorded... Trying latest installed.");
            let installed = self.find_freshest_install_tag();
            log::debug!("latest installed is {:?}", installed);
            let latest: &GitTag;
            if let Some(installed) = installed {
                latest = installed;
            } else {
                latest = self.releases.tags_list.first().unwrap();
            };

            let release = self.releases.get_release(latest).clone();
            self.activate_release(&release)?;
            return Ok(());
        } else if latest_good.is_none() {
            let mut ddays = 7;
            if let Some(args) = args {
                if args.ends_with("d") {
                    ddays = args[..args.len() - 1].parse()?
                }
            }
            println!("No good versions recorded. Trying {} days earlier", ddays);
            let earliest_tag = &self.track.0.last().unwrap().0;
            let approx_date = self.releases.find_tag(earliest_tag).tag_datetime().date();
            let earlier_date = approx_date - chrono::Days::new(ddays);
            let earlier_tag = select_earlier_release(&self.releases, Some(earlier_date))?;
            let earlier_release = self.releases.get_release(earlier_tag).clone();
            println!("found earlier release: {:?}", earlier_release.tag_name);
            return self.activate_release(&earlier_release);
        } else {
            let earliest_bad_tag = &self.releases.find_tag(earliest_bad.unwrap()).clone();
            let latest_good_tag = &self.releases.find_tag(latest_good.unwrap()).clone();
            let mut midpoint_tag =
                bisecting::select_midpoint_tag(&self.releases, latest_good_tag, earliest_bad_tag);
            if self.track.is_tag_skipped(midpoint_tag) {
                log::warn!("midpoint would be {:?}, but it's skipped", midpoint_tag);
                let old_mp = midpoint_tag;
                midpoint_tag =
                    bisecting::select_midpoint_tag(&self.releases, latest_good_tag, old_mp);
                if self.track.is_tag_skipped(midpoint_tag) {
                    log::warn!(
                        "midpoint would be {:?}, but it's skipped AGAIN",
                        midpoint_tag
                    );
                    midpoint_tag =
                        bisecting::select_midpoint_tag(&self.releases, old_mp, earliest_bad_tag);
                }
            }
            assert!(!self.track.is_tag_skipped(midpoint_tag));
            if midpoint_tag == earliest_bad_tag || midpoint_tag == latest_good_tag {
                // self.releases
                //     .maybe_fetch_releases(&[latest_good_tag, &earliest_bad_tag]);
                let good_rel = self.releases.get_release(latest_good_tag);
                let bad_rel = self.releases.get_release(earliest_bad_tag);
                println!(
                    "Bisected to commit range ( {} , {} ]\n  latest good - [{}]({})\n  earliest bad - [{}]({})",
                    &good_rel.target_commitish, &bad_rel.target_commitish,
                    good_rel.tag_name, good_rel.html_url, bad_rel.tag_name, bad_rel.html_url
                );
                return Ok(());
            }

            println!(
                "Approx. {} steps left.",
                bisecting::get_steps_left(&self.releases, latest_good_tag, earliest_bad_tag)
            );

            let release = self.releases.get_release(midpoint_tag).clone();
            return self.activate_release(&release);
        }
    }

    fn find_freshest_install_tag(&self) -> Option<&GitTag> {
        let re = regex::Regex::new(r"^.*-(\d{4}-\d{2}-\d{2}-\d{4})$").unwrap();
        let freshest_date = std::fs::read_dir(&self.config.unpack_dir)
            .ok()?
            .map(|x| x.unwrap())
            .filter(|x| x.file_type().unwrap().is_dir())
            .filter_map(|x| {
                re.captures(&x.file_name().to_string_lossy())
                    .map(|c| c[1].to_string())
            })
            .max();
        let freshest_tag =
            freshest_date.map(|x| self.releases.find_tag(&format!("cdda-experimental-{}", x)));
        log::debug!(
            "freshest = {:?} ; dir was {:?}",
            freshest_tag,
            self.config.unpack_dir
        );
        freshest_tag
    }

    fn activate_tag(&mut self, args: &str) -> anyhow::Result<()> {
        let mut want_tag_name = args;
        if args == "tip" {
            want_tag_name = &self.releases.tags_list.first().unwrap().name;
        } else if args == "recent" {
            want_tag_name = self.find_freshest_install_tag().unwrap().name.as_str();
        }
        let mut tag_name = self
            .releases
            .tags_list
            .iter()
            .find(|r| r.name.ends_with(want_tag_name));
        if tag_name.is_none() {
            log::warn!("Release {:?} not found. Trying substring", tag_name);
            tag_name = self
                .releases
                .tags_list
                .iter()
                .filter(|r| r.name.contains(&want_tag_name))
                .last();
        }
        if let Some(tag_name) = tag_name {
            let release = self.releases.get_release(tag_name);
            self.activate_release(&release.clone())?;
        } else {
            anyhow::bail!("Couldn't find tag {:?}", tag_name);
        }
        Ok(())
    }
    fn reset(&mut self) -> anyhow::Result<()> {
        self.track = Track::new();
        self.track.save()?;
        Ok(())
    }
    fn fix_font(&self) -> anyhow::Result<()> {
        std::fs::remove_file(self.config.userdata_dir.join("config").join("fonts.json")).ok();
        Ok(())
    }
}
fn interact() {
    let mut bisect_state = BisectState::new().unwrap();
    loop {
        println!("> ");
        let mut prompt: String = "".into();
        std::io::stdin().read_line(&mut prompt).unwrap();
        let mut it = prompt.trim().split(" ");
        let verb = it.next().unwrap();
        let args = it.next().map(|x| x.trim());
        let out = match verb {
            "fetch" => bisect_state.fetch_more_releases(),
            "launch" | "run" => bisect_state.launch(),
            "mark" => match args {
                Some("good") => bisect_state.mark_good(),
                Some("bad") => bisect_state.mark_bad(),
                Some("skip") => bisect_state.mark_skip(),
                Some("blacklist") => bisect_state.mark_blacklist(),
                _ => {
                    println!("?");
                    Ok(())
                }
            },
            "next" | "advance" => bisect_state.advance(args),
            "track" => bisect_state.show_track(),
            "activate" => bisect_state.activate_tag(args.unwrap()),
            "quit" | "exit" => break,
            "reset" => bisect_state.reset(),
            "fix_font" | "fix-font" => bisect_state.fix_font(),
            _ => {
                println!("?");
                Ok(())
            }
        };
        match out {
            Ok(()) => {}
            Err(e) => println!("Error: {:?}", e),
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("cdda_bisector=debug"))
        .init();

    println!("Hello, world!");
    interact();
    // get_all_releases(false).unwrap();
}
