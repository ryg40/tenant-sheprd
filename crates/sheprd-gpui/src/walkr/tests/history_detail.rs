use super::history_list::{Counting, record, sessions};
use crate::walkr::history::{Error, OptionBody, RATINGS_PAGE, detail, parse};
use std::{
    fs, io,
    time::{Duration, Instant},
};

const SESSION: &str = "20261101T120000Z-0123456789ab";
const LIMIT: u64 = 16 * 1024 * 1024;

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[test]
fn a_detail_reads_at_most_its_bound_and_one_byte() {
    let mut endless = Counting {
        inner: io::repeat(b' '),
        read: 0,
    };
    assert!(matches!(
        parse(SESSION, &mut endless, RATINGS_PAGE),
        Err(Error::TooLarge)
    ));
    assert!(endless.read <= LIMIT + 1);
}

#[test]
fn an_html_option_keeps_only_its_size() {
    const MARK: &str = "INVENTEDHTMLMARK";
    let options = format!(
        r#""options":[{{"optionId":"a","label":"A","format":"html","content":"<p>{MARK}</p><script>{MARK}</script>"}},{{"optionId":"b","label":"B","format":"text","content":"plain"}}]"#
    );
    let text = record(SESSION).replace(r#""options":[]"#, &options);
    let detail = parse(SESSION, text.as_bytes(), RATINGS_PAGE).unwrap();
    assert!(matches!(
        detail.options[0].body,
        OptionBody::NotDrawn { bytes } if bytes > 0
    ));
    assert!(matches!(&detail.options[1].body, OptionBody::Text { text, .. } if text == "plain"));
    // No field holds the HTML content.
    assert!(!format!("{detail:?}").contains(MARK));
}

#[test]
fn an_invalid_id_opens_nothing() {
    let home = tempfile::tempdir().unwrap();
    for id in ["../../etc", "..", "20261101T120000Z-0123456789ab/../x"] {
        assert!(matches!(
            detail(home.path(), id, RATINGS_PAGE, later()),
            Err(Error::InvalidId)
        ));
    }
}

#[cfg(unix)]
#[test]
fn a_detail_opens_no_record_outside_its_root() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let folder = outside.path().join(SESSION);
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("session.json"), record(SESSION)).unwrap();
    // A symlinked session folder.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(&root).unwrap();
    symlink(&folder, root.join(SESSION)).unwrap();
    assert!(matches!(
        detail(home.path(), SESSION, RATINGS_PAGE, later()),
        Err(Error::NotRegular)
    ));
    // A symlinked record.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.join(SESSION)).unwrap();
    symlink(
        folder.join("session.json"),
        root.join(SESSION).join("session.json"),
    )
    .unwrap();
    assert!(matches!(
        detail(home.path(), SESSION, RATINGS_PAGE, later()),
        Err(Error::NotRegular)
    ));
    // The same record in the root is read.
    let home = tempfile::tempdir().unwrap();
    let root = sessions(home.path());
    fs::create_dir_all(root.join(SESSION)).unwrap();
    fs::write(root.join(SESSION).join("session.json"), record(SESSION)).unwrap();
    assert!(detail(home.path(), SESSION, RATINGS_PAGE, later()).is_ok());
}

#[cfg(unix)]
#[test]
fn the_record_open_refuses_a_fifo_without_blocking() {
    use rustix::fs::{CWD, Mode, mkfifoat};
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("session.json");
    mkfifoat(CWD, &path, Mode::RUSR | Mode::WUSR).unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Call the open itself, bypassing every pre-open metadata check.
        let result = crate::walkr::history::open_record(CWD, &path);
        send.send(result).unwrap();
    });
    assert!(matches!(
        recv.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(Error::NotRegular)
    ));
    worker.join().unwrap();
}

#[test]
fn earlier_ratings_remain_available_in_larger_pages() {
    let ratings = (0..65)
        .map(|index| serde_json::json!({"value": 8, "note": format!("Rating {index}")}))
        .collect::<Vec<_>>();
    let mut record: serde_json::Value = serde_json::from_str(&record(SESSION)).unwrap();
    record["ratings"] = serde_json::json!(ratings);
    let bytes = serde_json::to_vec(&record).unwrap();
    let first = parse(SESSION, bytes.as_slice(), RATINGS_PAGE).unwrap();
    assert_eq!(first.ratings_total, ratings.len());
    assert_eq!(first.ratings.len(), RATINGS_PAGE);
    assert_eq!(
        first.ratings.first().unwrap().note.as_deref(),
        Some("Rating 1")
    );
    let expanded = parse(SESSION, bytes.as_slice(), RATINGS_PAGE * 2).unwrap();
    assert_eq!(expanded.ratings_total, ratings.len());
    assert_eq!(expanded.ratings.len(), ratings.len());
    assert_eq!(
        expanded.ratings.first().unwrap().note.as_deref(),
        Some("Rating 0")
    );
    assert_eq!(&expanded.ratings[1..], first.ratings);
}
