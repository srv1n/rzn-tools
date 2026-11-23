//! YouTube in `rzn.item/v2`: one item per video, one transcript block per caption segment with its
//! time range, and the chapter as the block's `path`.

use super::{
    parse_video_date, ListSource, ListVideosInput, ListVideosOutput, SearchResultItem,
    TimedTextSegment, VideoChapter, YouTubeVideoDetails,
};
use crate::error::ConnectorError;
use crate::item::{
    shared_item_fields, At, Block, Description, FieldDescription, FieldType, Item, ItemsPage,
    KindDescription, Listing, Party, Truncated,
};
use serde_json::json;

pub(super) fn video_key(video_id: &str) -> String {
    format!("youtube:video:{video_id}")
}

fn channel_party(name: &str, channel_id: Option<&str>) -> Party {
    let mut party = Party::named(name);
    if let Some(id) = channel_id.filter(|id| !id.is_empty()) {
        party = party
            .with_id(id)
            .with_url(format!("https://www.youtube.com/channel/{id}"));
    }
    party
}

/// The full item for one video. `transcript` is the fetched caption track, or the reason it could
/// not be fetched; without one the item says it is truncated, so an index does not take it.
pub(super) fn video_item(
    video_id: &str,
    info: &YouTubeVideoDetails,
    transcript: Result<(&[TimedTextSegment], &str, bool), &str>,
) -> Result<Item, ConnectorError> {
    let published_at = info
        .publish_date
        .as_deref()
        .and_then(parse_video_date)
        .or_else(|| info.upload_date.as_deref().and_then(parse_video_date))
        .ok_or_else(|| ConnectorError::Other(format!("video {video_id} has no publish date")))?;
    let channel = channel_party(&info.channel_name, Some(&info.channel_id));
    let mut item = Item::new(
        video_key(video_id),
        "video",
        format!("https://www.youtube.com/watch?v={video_id}"),
    )
    .with_title(info.title.clone())
    .with_author(channel.clone())
    .with_published_at(Some(published_at))
    .with_published_in(channel)
    .with_fields(json!({
        "description": info.description,
        "duration_s": info.duration_seconds,
        "tags": info.keywords,
        "is_live": info.is_live_content,
    }))
    .with_counts(json!({ "views": info.view_count }));

    match transcript {
        Ok((segments, language, generated)) => {
            item = item.with_fields(json!({
                "language": language,
                "captions": if generated { "generated" } else { "manual" },
            }));
            item.blocks = transcript_blocks(segments, &info.chapters);
            if item.blocks.is_empty() {
                item.truncated = Some(Truncated {
                    reason: "transcript_unavailable:empty_transcript".to_string(),
                    returned_blocks: 0,
                    total_blocks_hint: None,
                });
            }
        }
        Err(reason) => {
            item.truncated = Some(Truncated {
                reason: format!("transcript_unavailable:{reason}"),
                returned_blocks: 0,
                total_blocks_hint: None,
            });
        }
    }
    Ok(item)
}

/// One block per caption segment, in order, each with its time range and the chapter it falls in.
pub(super) fn transcript_blocks(
    segments: &[TimedTextSegment],
    chapters: &[VideoChapter],
) -> Vec<Block> {
    segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .enumerate()
        .map(|(index, segment)| {
            let mut block = Block::new(format!("t{index}"), "transcript", segment.text.clone());
            block.at = Some(At::Time {
                start_ms: segment.start_ms,
                end_ms: segment.start_ms.saturating_add(segment.duration_ms),
            });
            if let Some(chapter) = chapter_at(chapters, segment.start_ms) {
                block.path = vec![chapter.title.clone()];
                block
                    .fields
                    .insert("chapter".to_string(), json!(chapter.title));
            }
            block
        })
        .collect()
}

fn chapter_at(chapters: &[VideoChapter], start_ms: u64) -> Option<&VideoChapter> {
    let start_s = i64::try_from(start_ms / 1000).unwrap_or(i64::MAX);
    chapters
        .iter()
        .rev()
        .find(|chapter| i64::from(chapter.start_time) <= start_s)
}

/// A playlist or channel listing. It is `full` only when nothing narrowed it.
pub(super) fn listing_page(input_listing: Listing, out: &ListVideosOutput) -> ItemsPage {
    let items = out
        .entries
        .iter()
        .map(|video| {
            let channel = video
                .channel
                .as_deref()
                .or(video.channel_title.as_deref())
                .or(out.channel_title.as_deref());
            let mut item = Item::new(video_key(&video.id), "video", video.url.clone())
                .with_title(video.title.clone())
                .with_published_at(video.published_at.as_deref().and_then(parse_video_date))
                .with_fields(json!({ "playlist_title": video.playlist_title }));
            if let Some(name) = channel {
                let party = channel_party(name, video.channel_id.as_deref());
                item = item.with_author(party.clone()).with_published_in(party);
            }
            item
        })
        .collect();
    ItemsPage::new(items, None, input_listing)
}

pub(super) fn listing_kind(input: &ListVideosInput, source: ListSource) -> Listing {
    let narrowed = input.limit.is_some()
        || input.published_after.is_some()
        || input.published_within_days.is_some();
    match (source, narrowed) {
        (_, true) => Listing::Feed,
        (ListSource::Playlist | ListSource::Channel, false) => Listing::Full,
    }
}

/// Search results as a feed. Only videos are items; playlists and channels are scopes.
pub(super) fn search_page(results: &[SearchResultItem]) -> ItemsPage {
    let items = results
        .iter()
        .filter_map(|result| match result {
            SearchResultItem::Video(video) => {
                let mut item = Item::new(video_key(&video.id), "video", video.url.clone())
                    .with_title(video.title.clone())
                    .with_published_at(video.uploaded_at.as_deref().and_then(parse_video_date))
                    .with_fields(json!({ "duration_s": video.duration_seconds }))
                    .with_counts(json!({ "views": video.views }));
                if !video.channel_name.is_empty() {
                    let party = channel_party(&video.channel_name, None);
                    item = item.with_author(party.clone()).with_published_in(party);
                }
                Some(item)
            }
            SearchResultItem::Playlist(_) | SearchResultItem::Channel(_) => None,
        })
        .collect();
    ItemsPage::new(items, None, Listing::Feed)
}

pub(super) fn description() -> Description {
    let mut fields = shared_item_fields();
    fields.extend([
        FieldDescription::item("description", FieldType::String),
        FieldDescription::item("duration_s", FieldType::Number),
        FieldDescription::item("tags", FieldType::String).list(),
        FieldDescription::item("is_live", FieldType::Boolean),
        FieldDescription::item("language", FieldType::String),
        FieldDescription::item("captions", FieldType::String),
        FieldDescription::item("views", FieldType::Number).count(),
        FieldDescription::passage("chapter", FieldType::String),
    ]);
    Description::new(
        "youtube",
        vec![KindDescription {
            kind: "video".to_string(),
            identity: "youtube:video:<video id>".to_string(),
            listing: Listing::Full,
            scopes: vec!["playlist".to_string(), "channel".to_string()],
            list_tool: "list".to_string(),
            get_tool: "get".to_string(),
            fields,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{item_from_value, item_schema};

    fn details() -> YouTubeVideoDetails {
        YouTubeVideoDetails {
            title: "Sleep".into(),
            description: "About sleep".into(),
            channel_id: "UCabc".into(),
            channel_name: "Parenting Lab".into(),
            publish_date: Some("2026-01-02".into()),
            upload_date: None,
            duration_seconds: 120,
            view_count: 42,
            keywords: vec!["sleep".into(), "infant".into()],
            is_live_content: false,
            chapters: vec![
                VideoChapter {
                    title: "Intro".into(),
                    start_time: 0,
                },
                VideoChapter {
                    title: "Night weaning".into(),
                    start_time: 60,
                },
            ],
        }
    }

    fn segments() -> Vec<TimedTextSegment> {
        vec![
            TimedTextSegment {
                text: "Welcome back.".into(),
                start_ms: 0,
                duration_ms: 2500,
            },
            TimedTextSegment {
                text: "   ".into(),
                start_ms: 2500,
                duration_ms: 100,
            },
            TimedTextSegment {
                text: "Start night weaning at six months.".into(),
                start_ms: 61_000,
                duration_ms: 4000,
            },
        ]
    }

    #[test]
    fn video_item_carries_chapters_and_time_ranges() {
        let segments = segments();
        let item = video_item("vid1", &details(), Ok((&segments, "en", false))).unwrap();
        let value = serde_json::to_value(&item).unwrap();
        let checked = item_from_value(value.clone()).expect("valid full item");
        assert_eq!(checked.key, "youtube:video:vid1");
        assert_eq!(value["published_at"], "2026-01-02T00:00:00+00:00");
        assert_eq!(value["published_in"]["id"], "UCabc");
        assert_eq!(value["counts"]["views"], 42);
        assert_eq!(value["fields"]["captions"], "manual");
        assert_eq!(value["blocks"].as_array().unwrap().len(), 2);
        assert_eq!(value["blocks"][0]["path"], json!(["Intro"]));
        assert_eq!(
            value["blocks"][0]["at"],
            json!({"time": {"start_ms": 0, "end_ms": 2500}})
        );
        assert_eq!(value["blocks"][1]["path"], json!(["Night weaning"]));
        assert_eq!(value["blocks"][1]["fields"]["chapter"], "Night weaning");
        assert_eq!(value["blocks"][1]["at"]["time"]["start_ms"], 61_000);
        for block in value["blocks"].as_array().unwrap() {
            assert_eq!(block["kind"], "transcript");
            assert!(block["at"]["time"].is_object());
        }
        let required = item_schema()["required"].clone();
        for name in [
            "key",
            "kind",
            "url",
            "author",
            "published_at",
            "published_in",
        ] {
            assert!(value.get(name).is_some(), "{name} missing");
            assert!(required.as_array().unwrap().iter().any(|v| v == name));
        }
    }

    #[test]
    fn video_without_transcript_is_truncated_and_refused() {
        let item = video_item("vid1", &details(), Err("transcripts_disabled")).unwrap();
        assert!(item.truncated.is_some());
        assert!(item.check().is_err());
    }

    #[test]
    fn description_lists_identity_scopes_and_fields() {
        let value = serde_json::to_value(description()).unwrap();
        assert_eq!(value["type"], "rzn.describe/v2");
        let kind = &value["kinds"][0];
        assert_eq!(kind["identity"], "youtube:video:<video id>");
        assert_eq!(kind["scopes"], json!(["playlist", "channel"]));
        let fields = kind["fields"].as_array().unwrap();
        assert!(fields
            .iter()
            .any(|f| f["name"] == "views" && f["changes"] == true));
        assert!(fields
            .iter()
            .any(|f| f["name"] == "chapter" && f["of"] == "passage"));
    }
}
