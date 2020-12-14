use anyhow::{anyhow, Context, Result};
use dotenv::dotenv;
use reqwest::Client;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fmt::Display;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;
use tokio::prelude::*;
use tokio::time::{delay_for, Duration};

#[derive(Debug, Serialize, Deserialize)]
struct Artist {
    name: String,
    #[serde(deserialize_with = "from_str", serialize_with = "to_str")]
    playcount: u32,
    url: String,
}

fn from_str<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr,

    T::Err: Display,
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map_err(de::Error::custom)
}

fn to_str<S>(x: &u32, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&x.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
struct Tag {
    name: String,
    count: u32,
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TopTag {
    name: String,
    play_count: u32,
}

async fn get_top_artist(
    client: &Client,
    api_key: &str,
    user: &str,
    start_page: u64,
) -> Result<Vec<Artist>> {
    let mut artists = Vec::new();
    let mut current_page = start_page;
    loop {
        let url = format!("https://ws.audioscrobbler.com/2.0/?method=user.gettopartists&user={}&api_key={}&page={}&format=json", user, api_key, current_page);
        dbg!(&url);
        let mut response = client.get(&url).send().await?.json::<Value>().await?;
        let mut topartists = response
            .get_mut("topartists")
            .context("topartists key not found")?
            .take();
        let mut attr = topartists
            .get_mut("@attr")
            .context("@attr key not found")?
            .take();
        let page: u64 = attr
            .get_mut("page")
            .context("page key not found")?
            .take()
            .as_str()
            .context("page value is not a str")?
            .parse()?;
        let total_pages: u64 = attr
            .get_mut("totalPages")
            .context("totalPages key not found")?
            .take()
            .as_str()
            .context("totalPages value is not a str")?
            .parse()?;
        artists.append(&mut serde_json::from_value(
            topartists
                .get_mut("artist")
                .context("artist key not found")?
                .take(),
        )?);
        if page >= total_pages {
            break;
        }
        delay_for(Duration::from_secs(1)).await;
        current_page = page + 1;
    }
    Ok(artists)
}

async fn get_artist_top_tags(client: &Client, api_key: &str, artist: &str) -> Result<Vec<Tag>> {
    let url = format!("https://ws.audioscrobbler.com/2.0/?method=artist.gettoptags&artist={}&api_key={}&period=12month&format=json", urlencoding::encode(artist), api_key);
    dbg!(&url);
    let response = client.get(&url).send().await?;
    if response.status().is_success() {
        let mut json = response.json::<Value>().await?;
        Ok(serde_json::from_value(
            json.get_mut("toptags")
                .context("toptags key not found")?
                .take()
                .get_mut("tag")
                .context("tag key not found")?
                .take(),
        )?)
    } else {
        Err(anyhow!("Bad status: {}", response.status()))
    }
}

async fn load_artists(client: &Client, api_key: &str, user: &str) -> Result<Vec<Artist>> {
    let artists_path = Path::new("artists.json");
    if artists_path.exists() {
        Ok(serde_json::from_reader(&File::open(artists_path)?)?)
    } else {
        let artists = get_top_artist(&client, &api_key, &user, 1).await?;
        serde_json::to_writer(&File::create(artists_path)?, &artists)?;
        Ok(artists)
    }
}

async fn load_top_tags(client: &Client, api_key: &str, artists: &[Artist]) -> Result<Vec<TopTag>> {
    let tags_path = Path::new("tags.json");
    if tags_path.exists() {
        Ok(serde_json::from_reader(&File::open(tags_path)?)?)
    } else {
        let mut top_tags: HashMap<String, u32> = HashMap::new();
        for artist in artists {
            delay_for(Duration::from_secs(1)).await;
            if let Ok(tags) = get_artist_top_tags(&client, &api_key, &artist.name).await {
                for tag in tags.iter() {
                    *top_tags.entry(tag.name.clone()).or_insert(0) += artist.playcount;
                }
            } else {
                println!("Could not get top tags for artist: {}", artist.name);
            }
        }
        let mut top_tags: Vec<TopTag> = top_tags
            .into_iter()
            .map(|(name, play_count)| TopTag { name, play_count })
            .collect();
        top_tags.sort_unstable_by_key(|top_tag| top_tag.play_count);
        serde_json::to_writer(&File::create(tags_path)?, &top_tags)?;
        Ok(top_tags)
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let user = env::var("LASTFM_USER").expect("LASTFM_USER is defined in .env file");
    let api_key = env::var("LASTFM_API_KEY").expect("LASTFM_API_KEY is defined in .env file");
    let client = Client::new();
    let artists = load_artists(&client, &api_key, &user).await.unwrap();
    dbg!(artists.len());
    let top_tags = load_top_tags(&client, &api_key, &artists).await.unwrap();
    dbg!(&top_tags.len());
}
