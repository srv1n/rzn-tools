//! Hacker News in `rzn.item/v2`: one item per thread; the story text is the first block and every
//! comment is a block naming the comment it answers.

use super::types::{AlgoliaHit, HackerNewsItem, ItemType};
use super::{hn_created_at_hit, hn_created_at_item};
use crate::item::{Block, Item, Party};
use crate::utils::html_to_text;
use serde_json::json;

pub(super) fn thread_key(id: i64) -> String {
    format!("hackernews:item:{id}")
}

fn thread_url(id: i64) -> String {
    format!("https://news.ycombinator.com/item?id={id}")
}

fn hn_party() -> Party {
    Party::named("Hacker News")
        .with_id("news.ycombinator.com")
        .with_url("https://news.ycombinator.com")
}

fn user(name: &str) -> Party {
    Party::named(name)
        .with_id(name)
        .with_url(format!("https://news.ycombinator.com/user?id={name}"))
}

fn head(key: String, url: String, title: Option<&String>, author: Option<&String>) -> Item {
    let mut item = Item::new(key, "thread", url)
        .with_title(title.cloned().unwrap_or_default())
        .with_published_in(hn_party());
    if let Some(name) = author.filter(|name| !name.is_empty()) {
        item = item.with_author(user(name));
    }
    item
}

/// A search hit: a story is a thread entry; a comment hit names its own id.
pub(super) fn hit_entry(hit: &AlgoliaHit) -> Option<Item> {
    let id = hit.object_id.as_ref().and_then(|s| s.parse::<i64>().ok())?;
    let (key, kind) = if hit.is_story() {
        (thread_key(id), "thread")
    } else {
        (format!("hackernews:comment:{id}"), "comment")
    };
    let mut item = head(
        key,
        thread_url(id),
        hit.title.as_ref().or(hit.story_title.as_ref()),
        hit.author.as_ref(),
    )
    .with_published_at(hn_created_at_hit(hit))
    .with_fields(json!({ "link": hit.url, "story_id": hit.story_id, "parent_id": hit.parent_id }))
    .with_counts(json!({ "points": hit.points }));
    item.kind = kind.to_string();
    Some(item)
}

/// A front-page story as a listing entry.
pub(super) fn story_entry(id: i64, story: &HackerNewsItem) -> Item {
    head(
        thread_key(id),
        thread_url(id),
        story.title.as_ref(),
        story.author.as_ref(),
    )
    .with_published_at(hn_created_at_item(story))
    .with_fields(json!({ "link": story.url }))
    .with_counts(json!({ "points": story.points }))
}

/// A whole thread: the story text (when it has any) and every comment, depth first.
pub(super) fn thread_item(id: i64, story: &HackerNewsItem) -> Item {
    let story_id = story.id.unwrap_or(id);
    let mut item = story_entry(story_id, story);
    let text = html_to_text(story.text.as_deref().unwrap_or(""));
    if !text.trim().is_empty() {
        item.blocks
            .push(Block::new(format!("{story_id}"), "post", text));
    } else if let Some(title) = story.title.as_ref().filter(|t| !t.is_empty()) {
        item.blocks
            .push(Block::new(format!("{story_id}"), "post", title.clone()));
    }
    append_comments(story, story_id, 0, &mut item.blocks);
    item
}

fn append_comments(node: &HackerNewsItem, story_id: i64, depth: i64, blocks: &mut Vec<Block>) {
    for child in node.children.as_deref().unwrap_or(&[]) {
        if !matches!(child.r#type, Some(ItemType::Comment)) {
            continue;
        }
        let Some(id) = child.id else { continue };
        let text = html_to_text(child.text.as_deref().unwrap_or(""));
        if !text.trim().is_empty() {
            let mut block = Block::new(id.to_string(), "comment", text);
            block.url = Some(thread_url(id));
            block.author = child.author.as_deref().filter(|n| !n.is_empty()).map(user);
            block.published_at = hn_created_at_item(child);
            let parent = child.parent_id.unwrap_or(story_id).to_string();
            if blocks.iter().any(|b| b.id == parent) {
                block.parent = Some(parent);
            }
            block.fields = crate::item::fields(json!({ "depth": depth }));
            block.counts = crate::item::counts(json!({ "points": child.points }));
            blocks.push(block);
        }
        append_comments(child, story_id, depth.saturating_add(1), blocks);
    }
}
