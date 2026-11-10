//! A Forgejo account's worker answers as every worker does, but with no
//! checkout: without a token it reads and stores nothing, it grants no
//! access, and an action asked of it is refused.
use super::*;
use crate::{
    Action, RemoteRequest, RemoteScope, Service, SyncState, Timing, location::database_path_in,
};
use std::time::{Duration, Instant};

#[test]
fn without_a_token_the_account_reads_nothing_and_takes_no_action() {
    let dir = tempfile::tempdir().unwrap();
    let host = "forgejo.example.com";
    let mut request = RemoteRequest::new(RemoteScope::ForgejoAccount { host: host.into() });
    request.look_up_token = false;
    request.data_root = Some(dir.path().to_owned());
    request.timing = Timing {
        reload: Duration::from_millis(20),
        sync: Duration::from_secs(3600),
    };
    let service = Service::start_remote(request).unwrap();
    let snapshot = service::wait(&service, |snapshot| {
        matches!(snapshot.sources[0].state, SyncState::Failed(_))
    });
    let info = snapshot.repo.as_ref().unwrap();
    assert_eq!(
        info.repository,
        Repository::ForgejoAccount { host: host.into() }
    );
    assert_eq!(
        info.remote.as_ref().map(|remote| &remote.source),
        Some(&crate::forgejo::search::account_source(host))
    );
    assert!(snapshot.access.is_none(), "nothing listed can be acted on");
    assert!(snapshot.items.is_empty());
    let SyncState::Failed(error) = &snapshot.sources[0].state else {
        unreachable!()
    };
    assert!(matches!(
        **error,
        Error::Forgejo(crate::forgejo::Error::SignIn { ref host, .. }) if host == "forgejo.example.com"
    ));
    let database = database_path_in(
        dir.path(),
        &Repository::ForgejoAccount { host: host.into() },
    );
    assert!(!database.exists(), "nothing is stored without a token");

    assert!(service.act(Action::DeleteBead {
        source: crate::forgejo::search::account_source(host),
        id: "x".into(),
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    let notice = loop {
        if let Some(notice) = service.notices().pop() {
            break notice;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let Err(error) = notice.outcome else {
        panic!("an action was taken without a checkout");
    };
    assert!(matches!(
        *error,
        Error::Forgejo(crate::forgejo::Error::NoCheckout { ref host }) if host == "forgejo.example.com"
    ));
}
