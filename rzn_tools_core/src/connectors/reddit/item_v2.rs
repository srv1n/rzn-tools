//! Reddit in `rzn.item/v2`: one item per thread; the post body is the first block and every
//! comment is a block naming the block it answers.

use super::{RedditConnector, REDDIT_CANONICAL_BASE_URL};
use crate::item::{counts, fields, Block, Item, Party, Truncated};
use crate::utils::html_to_text;
use serde_json::{json, Value};

pub(super) fn thread_key(post_id: &str) -> String {
    format!("reddit:thread:{post_id}")
}

fn absolute(permalink: &str) -> Option<String> {
    if permalink.is_empty() {
        None
    } else if permalink.starts_with("http://") || permalink.starts_with("https://") {
        Some(permalink.to_string())
    } else {
        Some(format!("{REDDIT_CANONICAL_BASE_URL}{permalink}"))
    }
}

fn user(name: &str) -> Option<Party> {
    (!name.is_empty() && name != "[deleted]").then(|| {
        Party::named(format!("u/{name}"))
            .with_id(name)
            .with_url(format!("https://www.reddit.com/user/{name}/"))
    })
}

fn date(value: &Value) -> Option<String> {
    value.as_f64().and_then(RedditConnector::rfc3339_from_utc)
}

/// The head of a thread, as a listing entry or the start of a full item.
pub(super) fn post_entry(data: &Value) -> Item {
    let post_id = data["id"].as_str().unwrap_or("");
    let url = absolute(data["permalink"].as_str().unwrap_or(""))
        .unwrap_or_else(|| format!("{REDDIT_CANONICAL_BASE_URL}/comments/{post_id}"));
    let mut item = Item::new(thread_key(post_id), "thread", url)
        .with_title(data["title"].as_str().unwrap_or("").to_string())
        .with_published_at(date(&data["created_utc"]))
        .with_marker(
            data["edited"]
                .as_f64()
                .map(|edited| format!("edited:{edited}")),
        )
        .with_fields(json!({
            "flair": data["link_flair_text"],
            "domain": data["domain"],
            "link": data["url"],
            "is_self": data["is_self"],
            "over_18": data["over_18"],
        }))
        .with_counts(json!({
            "score": data["score"],
            "upvote_ratio": data["upvote_ratio"],
            "comments": data["num_comments"],
        }));
    if let Some(author) = user(data["author"].as_str().unwrap_or("")) {
        item = item.with_author(author);
    }
    if let Some(sub) = data["subreddit"].as_str().filter(|s| !s.is_empty()) {
        item = item.with_published_in(
            Party::named(format!("r/{sub}"))
                .with_id(sub)
                .with_url(format!("https://www.reddit.com/r/{sub}/")),
        );
    }
    item
}

/// A whole thread with its comments. When Reddit holds more comments than were fetched, the item
/// says so in `truncated`.
pub(super) fn thread_item(post: &Value, comments: &[Value], comment_limit: usize) -> Item {
    let mut item = post_entry(post);
    let post_id = post["id"].as_str().unwrap_or("").to_string();
    let selftext = post["selftext"].as_str().unwrap_or("");
    let body = if !selftext.is_empty() {
        selftext.to_string()
    } else {
        html_to_text(post["selftext_html"].as_str().unwrap_or(""))
    };
    let body = if body.trim().is_empty() {
        item.title.clone().unwrap_or_default()
    } else {
        body
    };
    if !body.trim().is_empty() {
        item.blocks.push(Block::new(post_id.clone(), "post", body));
    }
    let root = (!item.blocks.is_empty()).then_some(post_id);
    append_comments(comments, root.as_deref(), 0, &mut item.blocks);

    let total = post["num_comments"].as_u64().unwrap_or(0);
    if total > comment_limit as u64 {
        item.truncated = Some(Truncated {
            reason: "comment_limit".to_string(),
            returned_blocks: item.blocks.len() as u64,
            total_blocks_hint: Some(total + 1),
        });
    }
    item
}

fn append_comments(comments: &[Value], parent: Option<&str>, depth: i64, blocks: &mut Vec<Block>) {
    for comment in comments {
        let id = comment["id"].as_str().unwrap_or("");
        let text = comment["body"].as_str().unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let mut next_parent = parent;
        if !text.trim().is_empty() && text != "[deleted]" && text != "[removed]" {
            let mut block = Block::new(id, "comment", text);
            block.parent = parent.map(str::to_string);
            block.author = user(comment["author"].as_str().unwrap_or(""));
            block.published_at = date(&comment["created_utc"]);
            block.url = absolute(comment["permalink"].as_str().unwrap_or(""));
            block.fields = fields(json!({
                "depth": depth,
                "is_op": comment["is_submitter"],
                "distinguished": comment["distinguished"],
                "stickied": comment["stickied"],
            }));
            block.counts = counts(json!({ "score": comment["score"] }));
            blocks.push(block);
            next_parent = Some(id);
        }
        if let Some(replies) = comment["replies"].as_array() {
            append_comments(replies, next_parent, depth.saturating_add(1), blocks);
        }
    }
}

/// A user profile as an item with one summary block.
pub(super) fn user_item(username: &str, profile: &Value) -> Item {
    let karma = profile["total_karma"].as_i64().unwrap_or(0);
    let mut summary = format!("u/{username} — {karma} karma");
    if let Some(days) = profile["account_age_days"].as_i64() {
        summary.push_str(&format!(" — {days} days old"));
    }
    let mut item = Item::new(
        format!("reddit:user:{username}"),
        "user_profile",
        format!("https://www.reddit.com/user/{username}/"),
    )
    .with_title(format!("u/{username}"))
    .with_published_at(date(&profile["created_utc"]))
    .with_published_in(Party::named("Reddit").with_id("reddit.com"))
    .with_counts(json!({ "karma": karma }));
    if let Some(author) = user(username) {
        item = item.with_author(author);
    }
    item.blocks.push(Block::new("summary", "summary", summary));
    item
}
