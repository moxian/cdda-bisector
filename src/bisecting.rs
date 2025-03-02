use crate::release_hub::GitTag;
use chrono::Datelike;

use crate::ReleaseHub;

fn unclamp(val: usize, min: usize, max: usize) -> Option<usize> {
    assert!(min < max);
    // assert!(val != min && val != max, "{:?} {:?} {:?}", val, min, max);
    if min < val && val < max {
        return Some(val);
    } else {
        return None;
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Roundness {
    None,
    Day,
    Week,
    Month,
}

pub fn round_date(date: impl chrono::Datelike, roundness: Roundness) -> chrono::NaiveDate {
    let date = chrono::NaiveDate::from_yo_opt(date.year(), date.ordinal()).unwrap();
    match roundness {
        Roundness::None => return date,
        Roundness::Day => {
            return date;
        }
        Roundness::Week => {
            let point = date;
            let fake_weekstart = (point.day0() / 7) * 7 + 1; // fake because it's not on monday
            let point =
                chrono::NaiveDate::from_ymd_opt(point.year(), point.month(), fake_weekstart)
                    .unwrap();
            return point;
        }
        Roundness::Month => {
            let point = date;
            return chrono::NaiveDate::from_ymd_opt(point.year(), point.month(), 1).unwrap();
        }
    };
}

fn select_midpoint_rounded(
    tags: &[GitTag],
    good_old_pos: usize,
    bad_new_pos: usize,
    roundness: Roundness,
) -> Option<usize> {
    assert!(
        good_old_pos > bad_new_pos,
        "{:?} vs {:?}",
        good_old_pos,
        bad_new_pos
    );
    let naive_midpoint = (good_old_pos + bad_new_pos) / 2;
    let naive_dt = tags[naive_midpoint].tag_datetime();
    let round_before = round_date(naive_dt.date(), roundness);
    let round_after;
    match roundness {
        Roundness::None => return Some(naive_midpoint),
        Roundness::Day => {
            round_after = round_before + chrono::Duration::days(1);
        }
        Roundness::Week => {
            round_after = round_before + chrono::Duration::days(7);
        }
        Roundness::Month => {
            round_after = round_before + chrono::Months::new(1);
        }
    };
    // log::debug!("ndt rb ra = {:?} {:?} {:?}", naive_dt, round_before, round_after);
    let day_first = tags
        .iter()
        .enumerate()
        .filter(|(_, r)| r.tag_datetime().date() >= round_before)
        .last();
    let next_day_first = tags
        .iter()
        .enumerate()
        .filter(|(_, r)| r.tag_datetime().date() >= round_after)
        .last();
    let (Some(day_first), Some(next_day_first)) = (day_first, next_day_first) else {
        return None;
    };

    // log::debug!("pos: {} - {}", good_old_pos, bad_new_pos);
    // log::debug!(
    //     "day_first {:?} , next_day_first {:?}",
    //     (day_first.0, &day_first.1.tag_name),
    //     (next_day_first.0, &next_day_first.1.tag_name),
    // );
    let day_first_p = unclamp(day_first.0, bad_new_pos, good_old_pos);
    let next_day_first_p = unclamp(next_day_first.0, bad_new_pos, good_old_pos);
    // log::debug!(
    //     "day_first_p {:?} , next_day_first_p {:?}",
    //     day_first_p,
    //     next_day_first_p
    // );
    let (day_first_p, next_day_first_p) = match (day_first_p, next_day_first_p) {
        (None, None) => return None,
        (Some(v), None) | (None, Some(v)) => return Some(v),
        (Some(a), Some(b)) => (a, b),
    };
    let diff_before = naive_midpoint.abs_diff(day_first_p);
    let diff_after = naive_midpoint.abs_diff(next_day_first_p);
    // log::debug!("diff before {} , after {}", diff_before, diff_after);
    if diff_before < diff_after {
        return Some(day_first_p);
    } else {
        return Some(next_day_first_p);
    }
}

fn select_midpoint(tags: &[GitTag], good_old_pos: usize, bad_new_pos: usize) -> usize {
    // let mut mid = None;

    for r in [
        Roundness::Month,
        Roundness::Week,
        Roundness::Day,
        Roundness::None,
    ] {
        let m = select_midpoint_rounded(tags, good_old_pos, bad_new_pos, r);
        // log::debug!(
        //     "r {:?} - midpoint between {} and {} is {:?}",
        //     r,
        //     releases[good_old_pos].tag_name,
        //     releases[bad_new_pos].tag_name,
        //     m.map(|m| &releases[m].tag_name)
        // );
        if let Some(m) = m {
            // mid = Some(m)
            return m;
        }
    }
    // return mid.unwrap();
    unreachable!()
}

pub fn select_midpoint_tag<'a>(
    releases: &'a ReleaseHub,
    latest_good_tag: &GitTag,
    earliest_bad_tag: &GitTag,
) -> &'a GitTag {
    let (bad_pos, _bad_rel) = releases
        .tags_list
        .iter()
        .enumerate()
        .find(|(_, r)| r.name == earliest_bad_tag.name)
        .unwrap();
    let (good_pos, good_rel) = releases
        .tags_list
        .iter()
        .enumerate()
        .find(|(_, r)| r.name == latest_good_tag.name)
        .unwrap();
    if good_pos == bad_pos + 1 {
        return good_rel;
    }
    // let midpoint = (bad_pos + good_pos) / 2;

    let midpoint = select_midpoint(&releases.tags_list, good_pos, bad_pos);
    return &releases.tags_list[midpoint];
}

pub fn get_steps_left(releases: &ReleaseHub, latest_good_tag: &GitTag, earliest_bad_tag: &GitTag) -> i32 {
    let lg = releases
        .tags_list
        .iter()
        .enumerate()
        .find(|r| r.1.name == latest_good_tag.name)
        .unwrap();
    let eb = releases
        .tags_list
        .iter()
        .enumerate()
        .find(|r| r.1.name == earliest_bad_tag.name)
        .unwrap();
    let span = eb.0.abs_diff(lg.0);
    let steps = (span as f32).log2().ceil() as i32;
    return steps;
}

// pub fn select_next_tag_to_try<'a>(releases: &'a ReleaseHub, track: &crate::Track) -> &str{

// }
