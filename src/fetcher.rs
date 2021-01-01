use std::lazy::SyncLazy;

use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::{Error, Id, IdBuf, PlayerResponse, VideoDescrambler, VideoInfo};
use crate::video_info::player_response::playability_status::PlayabilityStatus;

/// A YouTubeFetcher, used to download all necessary data from YouTube, which then could be used
/// to extract video-urls, or other video information.
/// 
/// You will probably rarely use this type directly, and use [`YouTube`] instead. 
/// 
/// # Example
/// ```no_run
///# use rustube::{Id, VideoFetcher};
///# use url::Url;
/// const URL: &str = "https://youtube.com/watch?iv=5jlI4uzZGjU";
/// let url = Url::parse(URL).unwrap();
/// 
/// let fetcher: VideoFetcher =  VideoFetcher::from_url(&url).unwrap();
/// ```
#[derive(Clone, derive_more::Display, derivative::Derivative)]
#[display(fmt = "VideoFetcher({})", video_id)]
#[derivative(Debug, PartialEq, Eq)]
pub struct VideoFetcher {
    video_id: IdBuf,
    watch_url: Url,
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    client: Client,
}

impl VideoFetcher {
    /// Creates a YouTubeFetcher from an `Url`.
    /// For more information have a look at the `YouTube` documentation.
    /// # Errors
    /// This method fails if no valid video id can be extracted from the url or when `reqwest` fails
    /// to initialize an new `Client`.
    #[inline]
    pub fn from_url(url: &Url) -> crate::Result<Self> {
        let id = Id::from_raw(url.as_str())?
            .into_owned();
        Self::from_id(id)
    }

    /// Creates a YouTubeFetcher from an `Id`.
    /// For more information have a look at the `YouTube` documentation. 
    /// # Errors
    /// This method fails if `reqwest` fails to initialize an new `Client`.
    #[inline]
    pub fn from_id(video_id: IdBuf) -> crate::Result<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .build()?;
        Ok(Self::from_id_with_client(video_id, client))
    }

    /// Creates a YouTubeFetcher from an `Id` and an existing `Client`.
    /// There are no special constrains, what the `Client` has to look like.
    /// For more information have a look at the `YouTube` documentation.
    #[inline]
    pub fn from_id_with_client(video_id: IdBuf, client: Client) -> Self {
        Self {
            watch_url: video_id.watch_url(),
            video_id,
            client,
        }
    }

    /// Fetches all data necessary to extract important video information.
    /// For more information have a look at the `YouTube` documentation. 
    /// 
    /// # Errors
    /// This method fails, when the video is private, only for members, or otherwise not accessible,
    /// when it cannot request all necessary video resources, or when deserializing the raw response
    /// string into the corresponding Rust types fails.
    /// 
    /// When having a good internet connection, only errors due to inaccessible videos should occur.
    /// Other errors usually mean, that YouTube changed their API, and `rustube` did not adapt to 
    /// this change yet. Please feel free to open a GitHub issue if this is the case.
    /// 
    /// # How it works
    /// So you want to download a YouTube video? You probably already noticed, that YouTube makes 
    /// this quite hard, and does not just provide static urls for their videos. In fact, there's
    /// not the one url for each video. When currently nobody is watching a video, there's actually
    /// no url for this video at all!
    ///
    /// So we need to somehow show YouTube that we want to watch the video, so the YouTube server
    /// generates a url for us. To do this, we do what every 'normal' human being would do: we
    /// request the webpage of the video. To do so, we need nothing more, then the videos id (If you
    /// want to learn more about the id, you can have a look at the [`id`] module. But you don't
    /// need to know anything about it for now.). Let's say for example '5jlI4uzZGjU'. With this id,
    /// we can then visit <https://youtube.com/watch?v=5jlI4uzZGjU>, the site, you as a human, would
    /// go to when just watching the video.
    ///
    /// The next step is to extract as much information from <https://youtube.com/watch?v=5jlI4uzZGjU>
    /// as possible. This is, i.e., information like "is the video age restricted?", or "can we watch
    /// the video without being a member of that channel?".
    ///
    /// But there's information, which is a lot more important then knowing if we are old enough to
    /// to watch the video: The [`VideoInfo`], the [`PlayerResponse`], and the JavaScript of the 
    /// page. [`VideoInfo`] and [`PlayerResponse`] are JSON objects, which contain the most 
    /// important Information about the video. If you are feeling brave, feel free to have a look
    /// at the definitions of those two types, their subtypes, and all the information they contain
    /// (It's huge!). The JavaScript is not processed by fetch, but is used later by `descramble` to
    /// generate the `transform_plan` and the `transform_map` (you will learn about  both when it
    /// comes to descrambling).
    /// 
    /// To get the videos [`VideoInfo`], we actually need to request one more page, which you 
    /// usually probably don't visit as a 'normal' human being. Because we, programmers, are really
    /// creative when it comes to naming stuff, a videos [`VideoInfo`] can be requested at 
    /// <https://youtube.com/get_video_info>. Btw.: If you want to see how the computer feels, when 
    /// we ask him to deserialize the response into the [`VideoInfo`] struct, you can have a look
    /// at <https://www.youtube.com/get_video_info?video_id=5jlI4uzZGjU&eurl=https%3A%2F%2Fyoutube.com%2Fwatch%3Fiv%3D5jlI4uzZGjU&sts=>
    /// (most browsers, will download a text file!). This is the actual [`VideoInfo`] for the
    /// video with the id '5jlI4uzZGjU'. 
    /// 
    /// That's it! Of curse we are not even close to be able to download the video, yet. But that's
    /// not the task of `fetch`. `fetch` is just responsible for requesting all the important 
    /// information. To learn, how the journey continues, have a look at 
    /// [`YouTubeDescrambler::descramble`].  
    #[cfg(feature = "fetch")]
    pub async fn fetch(self) -> crate::Result<VideoDescrambler> {
        // fixme: It seems like watch_html also contains a PlayerResponse in all cases. VideoInfo
        // only contains the  extra field `adaptive_fmts_raw`. It may be possible to just use the 
        // watch_html PlayerResponse. This would eliminate one request and therefore improve 
        // performance.
        //
        // To do so, two things must happen:
        //      1. I need a video, which has `adaptive_fmts_raw` set, so I can examine
        //         both the watch_html as well as the video_info. (adaptive_fmts_raw even may be 
        //         a legacy thing, which isn't used by YouTube anymore).
        //      2. I need to have some kind of evidence, that watch_html comes with the 
        //         PlayerResponse in most cases. (It would also be possible to just check, weather
        //         or not watch_html contains PlayerResponse, and otherwise request video_info). 

        let watch_html = self.get_html(&self.watch_url).await?;
        let is_age_restricted = is_age_restricted(&watch_html);
        Self::check_availability(&watch_html, is_age_restricted)?;

        let (
            (js, player_response),
            mut video_info
        ) = tokio::try_join!(
            self.get_js(is_age_restricted, &watch_html),
            self.get_video_info(is_age_restricted)
        )?;

        match (&video_info.player_response.streaming_data, player_response) {
            (None, Some(player_response)) => video_info.player_response = player_response,
            (None, None) => return Err(Error::UnexpectedResponse(
                "StreamingData is none and the watch html did not contain a valid PlayerResponse".into()
            )),
            _ => {}
        }

        Ok(VideoDescrambler {
            video_info,
            client: self.client,
            js,
        })
    }

    #[inline]
    pub fn video_id(&self) -> Id {
        self.video_id.as_borrowed()
    }

    #[inline]
    pub fn watch_url(&self) -> &Url {
        &self.watch_url
    }

    fn check_availability(watch_html: &str, is_age_restricted: bool) -> crate::Result<()> {
        static PLAYABILITY_STATUS: SyncLazy<Regex> = SyncLazy::new(||
            Regex::new(r#"["']?playabilityStatus["']?\s*[:=]\s*"#).unwrap()
        );

        let playability_status: PlayabilityStatus = PLAYABILITY_STATUS
            .find_iter(watch_html)
            .map(|m| json_object(
                watch_html
                    .get(m.end()..)
                    .ok_or(Error::Internal("The regex does not match meaningful"))?
            ))
            .filter_map(Result::ok)
            .map(serde_json::from_str::<PlayabilityStatus>)
            .filter_map(Result::ok)
            .next()
            .ok_or_else(|| Error::UnexpectedResponse(
                "watch html did not contain a PlayabilityStatus".into()
            ))?;

        match playability_status {
            // maybe we can return the playability status, later skip it when deserializing
            // the PlayerResponse, and then use this one again?
            PlayabilityStatus::Ok { .. } => Ok(()),
            PlayabilityStatus::LoginRequired { .. } if is_age_restricted => Ok(()),
            ps => Err(Error::VideoUnavailable(ps))
        }
    }

    #[inline]
    #[cfg(feature = "fetch")]
    async fn get_js(&self,
                    is_age_restricted: bool,
                    watch_html: &str,
    ) -> crate::Result<(String, Option<PlayerResponse>)> {
        let (js_url, player_response) = match is_age_restricted {
            true => {
                let embed_url = self.video_id.embed_url();
                let embed_html = self.get_html(&embed_url).await?;
                js_url(&embed_html)?
            }
            false => js_url(watch_html)?
        };

        self
            .get_html(&js_url)
            .await
            .map(|html| (html, player_response))
    }

    #[inline]
    #[cfg(feature = "fetch")]
    async fn get_video_info(&self, is_age_restricted: bool) -> crate::Result<VideoInfo> {
        let video_info_url = self.get_video_info_url(is_age_restricted);
        let video_info_raw = self.get_html(&video_info_url).await?;

        let mut video_info = serde_qs::from_str::<VideoInfo>(video_info_raw.as_str())?;
        video_info.is_age_restricted = is_age_restricted;

        Ok(video_info)
    }

    #[inline]
    #[cfg(feature = "fetch")]
    fn get_video_info_url(&self, is_age_restricted: bool) -> Url {
        if is_age_restricted {
            video_info_url_age_restricted(
                self.video_id.as_borrowed(),
                &self.watch_url,
            )
        } else {
            video_info_url(
                self.video_id.as_borrowed(),
                &self.watch_url,
            )
        }
    }

    #[inline]
    #[cfg(feature = "fetch")]
    async fn get_html(&self, url: &Url) -> crate::Result<String> {
        Ok(
            self.client
                .get(url.as_str())
                .send()
                .await?
                .text()
                .await?
        )
    }
}

#[inline]
#[cfg(feature = "fetch")]
fn is_age_restricted(watch_html: &str) -> bool {
    static PATTERN: SyncLazy<Regex> = SyncLazy::new(|| Regex::new("og:restrictions:age").unwrap());
    PATTERN.is_match(watch_html)
}

#[inline]
#[cfg(feature = "fetch")]
fn video_info_url(video_id: Id, watch_url: &Url) -> Url {
    let params: &[(&str, &str)] = &[
        ("video_id", video_id.as_str()),
        ("ps", "default"),
        ("eurl", watch_url.as_str()),
        ("hl", "en_US")
    ];
    _video_info_url(params)
}

#[inline]
#[cfg(feature = "fetch")]
fn video_info_url_age_restricted(video_id: Id, watch_url: &Url) -> Url {
    static PATTERN: SyncLazy<Regex> = SyncLazy::new(|| Regex::new(r#""sts"\s*:\s*(\d+)"#).unwrap());

    let sts = match PATTERN.captures(watch_url.as_str()) {
        Some(c) => c.get(1).unwrap().as_str(),
        None => ""
    };

    let eurl = format!("https://youtube.googleapis.com/v/{}", video_id.as_str());
    let params: &[(&str, &str)] = &[
        ("video_id", video_id.as_str()),
        ("eurl", &eurl),
        ("sts", sts)
    ];
    _video_info_url(params)
}


#[inline]
#[cfg(feature = "fetch")]
fn _video_info_url(params: &[(&str, &str)]) -> Url {
    Url::parse_with_params(
        "https://youtube.com/get_video_info?",
        params,
    ).unwrap()
}

#[inline]
#[cfg(feature = "fetch")]
fn js_url(html: &str) -> crate::Result<(Url, Option<PlayerResponse>)> {
    let player_response = get_ytplayer_config(html);
    let base_js = match player_response {
        Ok(PlayerResponse { assets: Some(ref assets), .. }) => assets.js.as_str(),
        _ => get_ytplayer_js(html)?
    };

    Ok((Url::parse(&format!("https://youtube.com{}", base_js))?, player_response.ok()))
}

/// Get the YouTube player configuration data from the watch html.
/// 
/// Extract the ``ytplayer_config``, which is json data embedded within the
/// watch html and serves as the primary source of obtaining the stream
/// manifest data.
/// 
/// :param str html:
///     The html contents of the watch page.
/// :rtype: str
/// :returns:
///     Substring of the html containing the encoded manifest data.
#[inline]
#[cfg(feature = "fetch")]
fn get_ytplayer_config(html: &str) -> crate::Result<PlayerResponse> {
    static CONFIG_PATTERNS: SyncLazy<[Regex; 3]> = SyncLazy::new(|| [
        Regex::new(r"ytplayer\.config\s*=\s*").unwrap(),
        Regex::new(r"ytInitialPlayerResponse\s*=\s*").unwrap(),
        // fixme
        // pytube handles `setConfig` little bit differently. It parses the entire argument 
        // to `setConfig()` and then uses load json to find `PlayerResponse` inside of it.
        // We currently handle both the same way, and just deserialize into the `PlayerConfig` enum.
        // This *should* have the same effect.
        //
        // In the future, it may be a good idea, to also handle both cases differently, so we don't
        // loose performance on deserializing into an enum, but deserialize `CONFIG_PATTERNS` directly 
        // into `PlayerResponse`, and `SET_CONFIG_PATTERNS` into `Args`. The problem currently is, that
        // I don't know, if CONFIG_PATTERNS can also contain `Args`.
        Regex::new(r#"yt\.setConfig\(.*['"]PLAYER_CONFIG['"]:\s*"#).unwrap()
    ]);

    CONFIG_PATTERNS
        .iter()
        .find_map(|pattern| {
            let json = parse_for_object(html, pattern).ok()?;
            deserialize_ytplayer_config(json).ok()
        })
        .ok_or_else(|| Error::UnexpectedResponse(
            "Could not find ytplayer_config in the watch html.".into()
        ))
}

#[inline]
#[cfg(feature = "fetch")]
fn parse_for_object<'a>(html: &'a str, regex: &Regex) -> crate::Result<&'a str> {
    let json_obj_start = regex
        .find(html)
        .ok_or(Error::Internal("The regex does not match"))?
        .end();

    Ok(json_object(
        html
            .get(json_obj_start..)
            .ok_or(Error::Internal("The regex does not match meaningful"))?
    )?)
}

#[inline]
#[cfg(feature = "fetch")]
fn deserialize_ytplayer_config(json: &str) -> crate::Result<PlayerResponse> {
    #[derive(Deserialize)]
    struct Args { player_response: PlayerResponse }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PlayerConfig { Args { args: Args }, Response(PlayerResponse) }

    Ok(
        match serde_json::from_str::<PlayerConfig>(json)? {
            PlayerConfig::Args { args } => args.player_response,
            PlayerConfig::Response(pr) => pr
        }
    )
}

/// Get the YouTube player base JavaScript path.
/// 
/// :param str html
///     The html contents of the watch page.
/// :rtype: str
/// :returns:
///     Path to YouTube's base.js file.
#[inline]
#[cfg(feature = "fetch")]
fn get_ytplayer_js(html: &str) -> crate::Result<&str> {
    static JS_URL_PATTERNS: SyncLazy<Regex> = SyncLazy::new(||
        Regex::new(r"(/s/player/[\w\d]+/[\w\d_/.]+/base\.js)").unwrap()
    );

    match JS_URL_PATTERNS.captures(html) {
        Some(function_match) => Ok(function_match.get(1).unwrap().as_str()),
        None => Err(Error::UnexpectedResponse(format!(
            "could not extract the ytplayer-javascript url from the watch html",
        ).into()))
    }
}

#[inline]
#[cfg(feature = "fetch")]
fn json_object(mut html: &str) -> crate::Result<&str> {
    html = html.trim_start_matches(|c| c != '{');
    if html.is_empty() {
        return Err(Error::Internal("cannot parse a json object from an empty string"));
    }

    let mut stack = vec![b'{'];
    let mut skip = false;

    let (i, _c) = html
        .as_bytes()
        .iter()
        .enumerate()
        .skip(1)
        .find(
            |(_i, &curr_char)| find_json_object(curr_char, &mut skip, &mut stack)
        )
        .ok_or(Error::Internal("could not find a closing delimiter"))?;

    let full_obj = html
        .get(..=i)
        .expect("i must always mark the position of a valid '}' char");

    Ok(full_obj)
}

#[inline]
#[cfg(feature = "fetch")]
fn find_json_object(curr_char: u8, skip: &mut bool, stack: &mut Vec<u8>) -> bool {
    if *skip {
        *skip = false;
        return false;
    }

    let context = *stack
        .last()
        .expect("stack must start with len == 1, and find mut end, when len == 0");

    match curr_char {
        b'}' if context == b'{' => { stack.pop(); }
        b']' if context == b'[' => { stack.pop(); }
        b'"' if context == b'"' => { stack.pop(); }

        b'\\' if context == b'"' => { *skip = true; }

        b'{' if context != b'"' => stack.push(b'{'),
        b'[' if context != b'"' => stack.push(b'['),
        b'"' if context != b'"' => stack.push(b'"'),

        _ => {}
    }

    stack.is_empty()
}
