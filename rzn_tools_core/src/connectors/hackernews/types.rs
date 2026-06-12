use serde::{Deserialize, Serialize};

/// Represents the type of an item in Hacker News
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Job,
    Story,
    Comment,
    Poll,
    PollOpt,
    #[serde(other)]
    Unknown,
}

/// Represents an item in Hacker News (story, comment, job, poll, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackerNewsItem {
    /// The item's unique id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// The username of the item's author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Creation date of the item in ISO 8601 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Creation date of the item in Unix timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_i: Option<i64>,
    /// The type of item
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<ItemType>,
    /// The comment, story or poll text. HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The title of the story, poll or job. HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The URL of the story
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The story's score, or the votes for a pollopt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<i64>,
    /// The parent item's id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<i64>,
    /// The story id this item belongs to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_id: Option<i64>,
    /// Additional options (from Algolia API)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Nested children comments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<HackerNewsItem>>,
}

impl HackerNewsItem {}

/// Represents a Hacker News user
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct HackerNewsUser {
    /// The user's unique username. Case-sensitive.
    pub id: String,
    /// Creation date of the user, in Unix Time
    pub created: i64,
    /// The user's karma
    pub karma: i64,
    /// The user's optional self-description. HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// List of the user's stories, polls and comments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted: Option<Vec<i64>>,
}

/// Represents updates to items and profiles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct HackerNewsUpdates {
    /// List of updated item IDs
    pub items: Vec<i64>,
    /// List of updated profile IDs
    pub profiles: Vec<String>,
}

/// A simplified story response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleStory {
    pub id: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub comments: Vec<SimpleComment>,

    pub points: i64,
}

/// A simplified comment response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleComment {
    pub id: i64,
    pub text: String,
    pub author: Option<String>,
    pub created_at: String,
    pub parent_id: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<SimpleComment>,

    pub points: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SimpleItem {
    Story(SimpleStory),
    Comment(SimpleComment),
}

impl HackerNewsItem {
    pub fn into_simple(self) -> Option<SimpleItem> {
        match self.r#type {
            Some(ItemType::Story) => {
                let comments = self
                    .children
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|item| item.into_simple())
                    .filter_map(|item| match item {
                        SimpleItem::Comment(c) => Some(c),
                        _ => None,
                    })
                    .collect();

                Some(SimpleItem::Story(SimpleStory {
                    id: self.id.unwrap_or_default(),
                    title: self.title.unwrap_or_default(),
                    text: self.text,
                    url: self.url,
                    author: self.author,
                    created_at: self.created_at.unwrap_or_default(),
                    comments,
                    points: self.points.unwrap_or(0),
                }))
            }
            Some(ItemType::Comment) => {
                let children: Vec<SimpleComment> = self
                    .children
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|item| item.into_simple())
                    .filter_map(|item| match item {
                        SimpleItem::Comment(c) => Some(c),
                        _ => None,
                    })
                    .collect();

                Some(SimpleItem::Comment(SimpleComment {
                    id: self.id.unwrap_or_default(),
                    text: self.text.unwrap_or_default(),
                    author: self.author,
                    created_at: self.created_at.unwrap_or_default(),
                    parent_id: self.parent_id,
                    points: children.len() as i64,
                    children,
                }))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgoliaHit {
    #[serde(rename = "objectID")]
    pub object_id: Option<String>,
    #[serde(rename = "_tags")]
    pub tags: Option<Vec<String>>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub created_at_i: Option<i64>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub story_text: Option<String>,
    pub points: Option<i64>,
    pub parent_id: Option<i64>,
    pub story_id: Option<i64>,
    pub comment_text: Option<String>,
    pub story_title: Option<String>,
    pub story_url: Option<String>,
    pub children: Option<Vec<i64>>,
}

impl AlgoliaHit {
    // Helper method to determine if this hit is a story
    pub fn is_story(&self) -> bool {
        self.tags
            .as_ref()
            .map(|tags| tags.contains(&"story".to_string()))
            .unwrap_or(false)
    }

    // Helper method to determine if this hit is a comment
    pub fn is_comment(&self) -> bool {
        self.tags
            .as_ref()
            .map(|tags| tags.contains(&"comment".to_string()))
            .unwrap_or(false)
    }

    // Convert hit to a SimpleItem
    pub fn into_simple(self) -> Option<SimpleItem> {
        if self.is_story() {
            Some(SimpleItem::Story(SimpleStory {
                id: self
                    .object_id
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_default(),
                title: self.title.unwrap_or_default(),
                text: self.story_text,
                url: self.url,
                author: self.author,
                created_at: self.created_at.unwrap_or_default(),
                comments: Vec::new(), // Comments will be populated later if needed
                points: self.points.unwrap_or(0),
            }))
        } else if self.is_comment() {
            Some(SimpleItem::Comment(SimpleComment {
                id: self
                    .object_id
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_default(),
                text: self.comment_text.unwrap_or_default(),
                author: self.author,
                created_at: self.created_at.unwrap_or_default(),
                parent_id: self.parent_id,
                children: Vec::new(),
                points: self
                    .children
                    .map(|children| children.len() as i64)
                    .unwrap_or(0),
            }))
        } else {
            None
        }
    }
}
