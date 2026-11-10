//! A remote whose host `FORGEJO_HOST` names is read as Forgejo from the
//! next sync on, ahead of glab's say, without reopening the view.
use super::*;

#[test]
fn forgejo_host_makes_the_remote_a_forgejo_source() {
    let dir = tempfile::tempdir().unwrap();
    let (_service, mut worker, _host) = opened(dir.path());
    let source = |worker: &Worker| worker.snapshot.sources[0].clone();
    worker.recognize(Vec::new, || Some("other.example.com".into()));
    assert_eq!(source(&worker).key.provider, Provider::Github);

    // glab knows the host too; the setting naming it decides.
    let config = "hosts:\n    gitlab.acme.com:\n        ssh_host: github.acme.com\n";
    worker.recognize(
        || gitlab::cli_hosts::parse(config),
        || Some("github.acme.com".into()),
    );
    let status = source(&worker);
    let expected = SourceKey {
        provider: Provider::Forgejo,
        host: "github.acme.com".into(),
        repository: "group/app".into(),
    };
    assert_eq!(status.key, expected, "the host and path keep their key");
    assert!(matches!(status.state, SyncState::Syncing { .. }));
    assert!(
        sources(worker.snapshot.repo.as_ref().unwrap())[0].1,
        "this build reads it"
    );

    // Once Forgejo, neither is read again.
    worker.recognize(|| unreachable!(), || unreachable!());
    assert_eq!(source(&worker).key, expected);
}
