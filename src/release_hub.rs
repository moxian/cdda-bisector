use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

use anyhow::Context;

use crate::ReleaseBlacklist;

const GITHUB_API_ENDPOINT: &'static str = r"https://api.github.com";
const CDDA_REPO: &'static str = r"CleverRaven/Cataclysm-DDA";

#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Debug, Clone)]
pub struct GitTag {
    pub name: String,
    // datetime: chrono::NaiveDateTime,
}
impl GitTag {
    pub fn try_tag_datetime(&self) -> Option<chrono::NaiveDateTime> {
        for pat in [
            "cdda-experimental-%Y-%m-%d-%H%M",
            "cdda-experimental-%Y-%m-%d-%H-%M",
        ] {
            if let Ok(x) = chrono::NaiveDateTime::parse_from_str(&self.name, pat) {
                return Some(x);
            }
        }
        return None;
    }
    pub fn tag_datetime(&self) -> chrono::NaiveDateTime {
        self.try_tag_datetime()
            .with_context(|| format!("failed to parse datetime of tag {:?}", self.name))
            .unwrap()
    }
}

pub struct ReleaseHub {
    pub tags_list: Vec<GitTag>,
    tags_info: RefCell<HashMap<GitTag, GithubRelease>>,
    // releases: Vec<GithubRelease>,
    blacklist: ReleaseBlacklist,
    client: ApiClient,
}
impl ReleaseHub {
    pub fn find_tag<'a>(&'a self, tag: &str) -> &'a GitTag {
        self.tags_list.iter().find(|r| r.name == tag).unwrap()
    }
    pub fn load() -> anyhow::Result<Self> {
        // let git_path = git_repo_path.into();
        let blacklist = ReleaseBlacklist::load().with_context(|| format!("loading blacklist"))?;

        // let inner = trim_releases(get_all_releases(false)?);
        let mut out = Self {
            tags_list: Default::default(),
            tags_info: Default::default(),
            // releases: inner,
            blacklist,
            client: ApiClient::new(),
        };
        out.fetch_more_releases().with_context(|| format!("fetching releases"))?;
        Ok(out)
    }

    pub fn fetch_more_releases(&mut self) -> anyhow::Result<()> {
        let bad_set = self
            .blacklist
            .release_tags
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let tags_list = git_grab_tags_list();
        let tags_list = tags_list
            .into_iter()
            .filter(|x| !bad_set.contains(&x.name))
            .collect::<Vec<_>>();
        self.tags_list = tags_list;
        Ok(())
    }
    pub fn mark_blacklist(&mut self, release: &GithubRelease) -> anyhow::Result<()> {
        self.blacklist.add(release)
    }
    // fn maybe_fetch_releases(&mut self, tags: &[&GitTag]) {
    //     for tag in tags {
    //         if !self.tags_info.contains_key(&tag) {
    //             self.tags_info.insert(
    //                 (*tag).clone(),
    //                 self.client.get_release_info(&tag.name).unwrap(),
    //             );
    //         }
    //     }
    // }
    pub fn get_release(&self, tag: &GitTag) -> GithubRelease {
        if !self.tags_info.borrow().contains_key(&tag) {
            self.tags_info.borrow_mut().insert(
                (*tag).clone(),
                self.client.get_release_info(&tag.name).unwrap(),
            );
        }
        self.tags_info.borrow().get(tag).unwrap().clone()
    }
}

fn git_grab_tags_list() -> Vec<GitTag> {
    let out = std::process::Command::new("git")
        .args(["ls-remote", "--tags", "--refs", "--quiet"])
        .arg("https://github.com/CleverRaven/Cataclysm-DDA.git")
        .arg("cdda-experimental-*-*")
        .output()
        .unwrap();
    let mut tags = vec![];
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let (_hash, tag) = line.split_once("\t").unwrap();
        let tag = tag.trim();
        let actual_tag = GitTag {
            name: tag.split("/").last().unwrap().to_string(),
        };
        if actual_tag.try_tag_datetime().is_none() {
            continue;
        }

        tags.push(actual_tag);
    }
    tags.sort_by_key(|t| std::cmp::Reverse(t.name.to_owned()));
    println!(
        "Got {} releases, latest one being {:?}",
        tags.len(),
        tags.first().map(|x| x.name.as_str()).unwrap_or("???")
    );
    tags
}
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GithubRelease {
    pub id: i64,
    pub published_at: String,
    pub tag_name: String,
    pub assets: Vec<ReleaseAsset>,
    //pub url: String,
    pub html_url: String,
    pub target_commitish: String,
}
impl GithubRelease {}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(serde::Deserialize, Debug)]
struct GithubCommitParent {
    sha: String,
}
#[derive(serde::Deserialize, Debug)]
struct GithubCommit {
    #[allow(dead_code)]
    sha: String,
    parents: Vec<GithubCommitParent>,
}

struct ApiClient {
    agent: ureq::Agent,
}
impl ApiClient {
    fn new() -> Self {
        Self {
            agent: ureq::builder().user_agent("moxian-bisector-thingy").build(),
        }
    }
    fn get_release_info(&self, release_tag: &str) -> anyhow::Result<GithubRelease> {
        let url = format!(
            "{}/repos/{}/releases/tags/{}",
            GITHUB_API_ENDPOINT, CDDA_REPO, release_tag
        );
        let thing = self.agent.get(&url).call()?.into_string()?;
        let release: GithubRelease = serde_json::from_str(&thing)?;
        Ok(release)
    }
    fn get_release_list(&self, page: i32) -> anyhow::Result<Vec<GithubRelease>> {
        let url = format!("{}/repos/{}/releases", GITHUB_API_ENDPOINT, CDDA_REPO);
        let thing = self
            .agent
            .get(&url)
            .query("per_page", "30")
            .query("page", &page.to_string())
            .call()?
            .into_string()?;
        let releases: Vec<GithubRelease> = serde_json::from_str(&thing)?;
        return Ok(releases);
    }
    fn get_parent_hash(&self, target: &str) -> anyhow::Result<String> {
        let url = format!(
            "{}/repos/{}/commits/{}",
            GITHUB_API_ENDPOINT, CDDA_REPO, target
        );
        let thing = self
            .agent
            .get(&url)
            .query("per_page", "1")
            .call()?
            .into_string()?;

        let response: GithubCommit = serde_json::from_str(&thing)?;
        anyhow::ensure!(response.parents.len() == 1, "{:?}", response);
        Ok(response.parents[0].sha.clone())
    }
}

fn get_release_page(page: i32) -> anyhow::Result<Vec<GithubRelease>> {
    log::info!("Fetching releases page {}", page);

    let agent = ApiClient::new();
    let releases = agent.get_release_list(page)?;
    anyhow::ensure!(releases.len() > 0);
    Ok(releases)
}

fn get_all_releases(network: bool) -> anyhow::Result<Vec<GithubRelease>> {
    let latest_known_id: i64; // hardcoded because yay
    let releases_cache_file = std::path::Path::new("cache/releases.json");
    let mut all_releases: Vec<GithubRelease>;
    if releases_cache_file.exists() {
        all_releases = serde_json::from_str(&std::fs::read_to_string(releases_cache_file)?)?;
        latest_known_id = all_releases[0].id;
    } else {
        all_releases = vec![];
        latest_known_id = 168620093;
    }

    if network {
        'pagination: for cur_page in 1.. {
            let page_releases = get_release_page(cur_page)?;

            for new_release in page_releases {
                if new_release.id == latest_known_id {
                    break 'pagination;
                }
                if all_releases.iter().any(|r| r.id == new_release.id) {
                    continue;
                }
                all_releases.push(new_release)
            }
        }

        std::fs::create_dir_all(releases_cache_file.parent().unwrap())?;
        std::fs::File::create(releases_cache_file)?
            .write_all(serde_json::to_string_pretty(&all_releases)?.as_bytes())?;
    }
    println!("got {} releases", all_releases.len());
    Ok(all_releases)
}

fn fetch_more_releases() -> anyhow::Result<()> {
    let extra_pages = 5;
    let releases_cache_file = std::path::Path::new("cache/releases.json");
    let mut all_releases: Vec<GithubRelease>;
    if releases_cache_file.exists() {
        all_releases = serde_json::from_str(&std::fs::read_to_string(releases_cache_file)?)?;
    } else {
        all_releases = vec![];
    }

    // fetch front
    let mut caught_up = false;
    let mut new_releases = vec![];
    for cur_page in 1.. {
        let page_of_releases = get_release_page(cur_page)?;

        for new_release in page_of_releases {
            if all_releases.iter().any(|ar| ar.id == new_release.id) {
                caught_up = true;
                break;
            } else {
                new_releases.push(new_release);
            }
        }
        if all_releases.is_empty() {
            caught_up = true;
        }

        if caught_up {
            break;
        }
    }
    let had_any_new = !new_releases.is_empty();
    all_releases = new_releases
        .into_iter()
        .chain(all_releases)
        .collect::<Vec<_>>();

    if !had_any_new {
        // now find the back
        let page_size = 30;
        let approx_start_page = all_releases.len() as i32 / page_size;
        for cur_page in approx_start_page..approx_start_page + extra_pages {
            let page_of_releases = get_release_page(cur_page)?;
            let new_releases = page_of_releases
                .into_iter()
                .filter(|pr| !all_releases.iter().any(|ar| ar.id == pr.id))
                .collect::<Vec<_>>();
            all_releases.extend(new_releases);
        }
    }

    all_releases.sort_by_key(|r| -r.id);

    std::fs::create_dir_all(releases_cache_file.parent().unwrap())?;
    std::fs::File::create(releases_cache_file)?
        .write_all(serde_json::to_string_pretty(&all_releases)?.as_bytes())?;

    println!("got {} releases", all_releases.len());
    Ok(())
}

fn trim_releases(releases: Vec<GithubRelease>) -> Vec<GithubRelease> {
    let out = releases
        .into_iter()
        .filter(|r| r.tag_name.starts_with("cdda-experimental"))
        .filter(|r| {
            r.assets.iter().any(|a| {
                a.name.starts_with("cdda-windows-tiles")
                    || a.name.starts_with("cdda-windows-with-graphics")
            })
        })
        .collect::<Vec<_>>();
    out
}

#[allow(dead_code)]
pub fn get_parent_commit(commit: &str) -> anyhow::Result<String> {
    let agent = ApiClient::new();
    agent.get_parent_hash(commit)
}
