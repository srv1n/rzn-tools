use rzn_tools_core::resolver::SmartResolver;

#[test]
fn test_biorxiv_patterns() {
    let resolver = SmartResolver::new();

    // URL with biorxiv
    let action = resolver
        .resolve("https://www.biorxiv.org/content/10.1101/2023.12.01.569584v1")
        .unwrap();
    assert_eq!(action.connector, "biorxiv");
    assert_eq!(action.tool, "get");
    assert_eq!(
        action.arguments.get("doi").unwrap(),
        "10.1101/2023.12.01.569584v1"
    );
    assert_eq!(action.arguments.get("server").unwrap(), "biorxiv");

    // URL with medrxiv
    let action = resolver
        .resolve("https://www.medrxiv.org/content/10.1101/2023.12.01.569584v1")
        .unwrap();
    assert_eq!(action.connector, "biorxiv");
    assert_eq!(action.arguments.get("server").unwrap(), "medrxiv");

    // DOI pattern
    let action = resolver
        .resolve("biorxiv:10.1101/2023.12.01.569584")
        .unwrap();
    assert_eq!(action.connector, "biorxiv");
    assert_eq!(
        action.arguments.get("doi").unwrap(),
        "10.1101/2023.12.01.569584"
    );
    assert_eq!(action.arguments.get("server").unwrap(), "biorxiv");
}

#[test]
fn test_rss_patterns() {
    let resolver = SmartResolver::new();

    let action = resolver.resolve("https://example.com/feed.xml").unwrap();
    assert_eq!(action.connector, "rss");
    assert_eq!(action.tool, "get_feed");
    assert_eq!(
        action.arguments.get("url").unwrap(),
        "https://example.com/feed.xml"
    );

    let action = resolver.resolve("https://example.com/blog.rss").unwrap();
    assert_eq!(action.connector, "rss");
}

#[test]
fn test_discord_patterns() {
    let resolver = SmartResolver::new();

    let action = resolver
        .resolve("https://discord.com/channels/1234567890/9876543210")
        .unwrap();
    assert_eq!(action.connector, "discord");
    assert_eq!(action.tool, "read_messages");
    assert_eq!(action.arguments.get("channel_id").unwrap(), "9876543210");
}

#[test]
fn test_doi_resolves_to_both_semantic_scholar_and_scihub() {
    let resolver = SmartResolver::new();

    let actions = resolver.resolve_all("10.1038/nature12373");
    let connectors: Vec<&str> = actions.iter().map(|a| a.connector.as_str()).collect();
    assert!(
        connectors.contains(&"semantic-scholar"),
        "Expected semantic-scholar in results, got: {:?}",
        connectors
    );
    assert!(
        connectors.contains(&"scihub"),
        "Expected scihub in results, got: {:?}",
        connectors
    );

    // Verify scihub action has correct tool and arg name
    let scihub_action = actions.iter().find(|a| a.connector == "scihub").unwrap();
    assert_eq!(scihub_action.tool, "get");
    assert_eq!(
        scihub_action.arguments.get("doi").unwrap(),
        "10.1038/nature12373"
    );
}

#[test]
fn test_doi_url_resolves_to_both_semantic_scholar_and_scihub() {
    let resolver = SmartResolver::new();

    let actions = resolver.resolve_all("https://doi.org/10.1038/nature12373");
    let connectors: Vec<&str> = actions.iter().map(|a| a.connector.as_str()).collect();
    assert!(
        connectors.contains(&"semantic-scholar"),
        "Expected semantic-scholar in results, got: {:?}",
        connectors
    );
    assert!(
        connectors.contains(&"scihub"),
        "Expected scihub in results, got: {:?}",
        connectors
    );

    // Verify scihub action maps DOI correctly (as "doi", not "paper_id")
    let scihub_action = actions.iter().find(|a| a.connector == "scihub").unwrap();
    assert_eq!(scihub_action.tool, "get");
    assert!(scihub_action.arguments.contains_key("doi"));
    assert!(!scihub_action.arguments.contains_key("paper_id"));

    // Verify semantic-scholar maps DOI as "paper_id"
    let ss_action = actions
        .iter()
        .find(|a| a.connector == "semantic-scholar")
        .unwrap();
    assert!(ss_action.arguments.contains_key("paper_id"));
}

#[test]
fn test_reddit_user_url_routes_to_user_tool() {
    let resolver = SmartResolver::new();

    let action = resolver
        .resolve("https://www.reddit.com/user/spez/")
        .unwrap();
    assert_eq!(action.connector, "reddit");
    assert_eq!(action.tool, "user");
    assert_eq!(action.arguments.get("username").unwrap(), "spez");
}
