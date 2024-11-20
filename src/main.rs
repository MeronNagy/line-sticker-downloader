use regex::Regex;
use reqwest;
use scraper::{Html, Selector};
use std::collections::HashSet;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <url1> <url2> ...", args[0]);
        std::process::exit(1);
    }

    for url in &args[1..] {
        if Url::parse(url).is_ok() {
            println!("Fetching {}", url);
            download_stickers(url).await?;
        } else {
            eprintln!("{} is not a url skipping.", url);
        }
    }

    Ok(())
}

async fn download_stickers(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?.text().await?;
    let document = Html::parse_document(resp.as_str());

    let directory = get_title(&document);
    println!("Creating dir: {}", directory);
    std::fs::create_dir_all(&directory)?;

    let selector = Selector::parse(r#"span.mdCMN09Image"#).unwrap();

    let mut img_urls = HashSet::new();
    for (_, element) in document.select(&selector).enumerate() {
        if let Some(style) = element.value().attr("style") {
            img_urls.insert(extract_image_url(style));
        }
    }

    for img_url in img_urls {
        let re = Regex::new(r"/(\d+)/").unwrap();
        let id = re.captures(&*img_url).unwrap().get(1).unwrap().as_str();
        let file_path = format!("{}/{:03}.png", directory, id);

        if std::path::Path::new(&file_path).exists() {
            println!("File already downloaded: {}", file_path);
            continue;
        }

        println!("Downloading: {}", img_url);
        let img_resp = client.get(&img_url).send().await?;
        let img_bytes = img_resp.bytes().await?;
        std::fs::write(&file_path, img_bytes)?;
    }

    Ok(())
}

fn get_title(document: &Html) -> String {
    let selector = Selector::parse(r#"p[data-test="sticker-name-title"]"#).unwrap();

    let element = document.select(&selector).next().unwrap();
    let text = element.text().next().unwrap();

    text.to_string()
}

fn extract_image_url(style: &str) -> String {
    let url_start = style.find("https").unwrap();

    style[url_start..].split("?").next().unwrap().to_string()
}
