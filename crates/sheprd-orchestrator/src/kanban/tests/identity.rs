use super::*;

#[test]
fn host_identity_and_checkout_preference_keep_the_concrete_move_target() -> Result<()> {
    let first = source(Provider::Forgejo, "forgejo.example.test", "Team/App");
    let second = source(Provider::Forgejo, "forgejo.example.test:8443", "Team/App");
    let account_source = source(Provider::Forgejo, "FORGEJO.EXAMPLE.TEST", "team/app");
    let remote_only = source(Provider::Forgejo, "forgejo.example.test", "team/remote");
    let github = source(Provider::Github, "github.example.test", "Team/App");
    let first_item = issue(&first, "7", "open", &[]);
    let second_item = issue(&second, "7", "closed", &[]);
    let account_item = issue(&account_source, "7", "open", &["status: in-review"]);
    let remote_item = issue(&remote_only, "7", "open", &[]);
    let github_item = issue(&github, "7", "open", &[]);
    let first_route = checkout(&first, 11);
    let second_route = checkout(&second, 22);
    let github_route = checkout(&github, 33);
    let account_route = Route {
        origin: Origin::ForgejoAccount {
            host: "forgejo.example.test".into(),
        },
        worker: 44,
    };
    let facts = complete();
    let mappings = Mappings::default();
    for reversed in [false, true] {
        let mut inputs = vec![
            CardInput {
                facts: &facts,
                item: &account_item,
                route: &account_route,
            },
            CardInput {
                facts: &facts,
                item: &first_item,
                route: &first_route,
            },
            CardInput {
                facts: &facts,
                item: &second_item,
                route: &second_route,
            },
            CardInput {
                facts: &facts,
                item: &remote_item,
                route: &account_route,
            },
            CardInput {
                facts: &facts,
                item: &github_item,
                route: &github_route,
            },
        ];
        if reversed {
            inputs.reverse();
        }
        let board = project([], inputs, &mappings, |_, _| true)?;
        let targets: std::collections::HashSet<_> = cards(&board)
            .map(|card| (&card.key.item, card.route.worker))
            .collect();
        assert_eq!(
            targets,
            [
                (&first_item.key, 11),
                (&second_item.key, 22),
                (&remote_item.key, 44),
                (&github_item.key, 33)
            ]
            .into_iter()
            .collect()
        );
        assert_ne!(
            CardKey::try_from(&first_item.key)?,
            CardKey::try_from(&second_item.key)?
        );
        assert_eq!(
            LaneKey::try_from(&first)?,
            LaneKey::try_from(&account_source)?
        );
    }
    for alias in [
        source(Provider::Github, "github.example.test", "team/app"),
        source(Provider::Github, "GITHUB.EXAMPLE.TEST", "team/app"),
    ] {
        let alias_item = issue(&alias, "7", "open", &[]);
        let alias_route = checkout(&alias, 55);
        let lane = LaneKey::try_from(&github)?;
        assert_eq!(lane, LaneKey::try_from(&alias)?);
        for selected in [0, 1] {
            let records = [(&github_item, &github_route), (&alias_item, &alias_route)];
            let board = project(
                [],
                [records[selected], records[1 - selected]]
                    .into_iter()
                    .map(|(item, route)| CardInput {
                        facts: &facts,
                        item,
                        route,
                    }),
                &mappings,
                |_, _| true,
            )?;
            assert_eq!(
                board.lanes.iter().map(|lane| &lane.key).collect::<Vec<_>>(),
                vec![&lane]
            );
            let (item, route) = records[selected];
            assert_eq!(
                cards(&board)
                    .map(|card| (&card.key.item, &card.item, &card.route))
                    .collect::<Vec<_>>(),
                vec![(&item.key, item, route)]
            );
        }
    }
    let board = project(
        [LaneInput {
            source: &first,
            route: &first_route,
        }],
        [CardInput {
            facts: &facts,
            item: &account_item,
            route: &account_route,
        }],
        &mappings,
        |_, _| true,
    )?;
    assert!(cards(&board).next().is_none());
    let first_lane = LaneKey::try_from(&first)?;
    assert!(board.lanes.iter().any(|lane| lane.key == first_lane));
    assert!(matches!(
        project(
            [],
            [CardInput {
                facts: &facts,
                item: &second_item,
                route: &first_route
            }],
            &mappings,
            |_, _| true
        ),
        Err(Error::Route)
    ));
    assert!(matches!(
        project(
            [],
            [CardInput {
                facts: &facts,
                item: &second_item,
                route: &account_route
            }],
            &mappings,
            |_, _| true
        ),
        Err(Error::Route)
    ));
    Ok(())
}
