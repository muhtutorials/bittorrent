use crate::dot_torrent::DotTorrent;
use anyhow::{Context, anyhow};
use hex;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

// NOTE: `info_hash` field is not included.
// Added separately to the URL parameters because
// libraries escape our serialization of it and mess it up
#[derive(Debug, Clone, Serialize)]
pub struct TrackerRequest {
    // The 20 byte sha1 hash of the bencoded form
    // of the info value from the metainfo file.
    // This value will almost certainly have to be escaped.
    // #[serde(serialize_with = "url_encode")]
    // pub info_hash: [u8; 20],

    // A string of length 20 which this downloader uses as its id.
    // Each downloader generates its own id at random
    // at the start of a new download.
    // This value will also almost certainly have to be escaped.
    // pub peer_id: String,

    // The port number this peer is listening on.
    // Common behavior is for a downloader to try
    // to listen on port 6881 and if that port is taken
    // try 6882, then 6883, etc. and give up after 6889.
    pub port: u16,

    // The total amount uploaded so far, encoded in base ten ASCII.
    pub uploaded: usize,

    // The total amount downloaded so far, encoded in base ten ASCII.
    pub downloaded: usize,

    // The number of bytes this peer still has to download,
    // encoded in base ten ASCII. Note that this can't be
    // computed from downloaded and the file length since
    // it might be a resume, and there's a chance that some
    // of the downloaded data failed an integrity check
    // and had to be re-downloaded.
    pub left: usize,

    // Setting this to 1 indicates that the client accepts
    // a compact response. The peers list is replaced by
    // a peers string with 6 bytes per peer. The first four
    // bytes are the host (in network byte order), the last
    // two bytes are the port (again in network byte order).
    // It should be noted that some trackers only support
    // compact responses (for saving bandwidth) and either
    // refuse requests without "compact=1" or simply send
    // a compact response unless the request contains
    // "compact=0" (in which case they will refuse the request.)
    pub compact: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    // Interval in seconds that the client should wait
    // between sending regular requests to the tracker
    pub interval: usize,

    // peers value may be a string consisting of multiples of 6 bytes.
    // First 4 bytes are the IP address and last 2 bytes are
    // the port number. All in network (big endian) notation.
    pub peers: PeerList,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponseErr {
    reason: String,
}

pub async fn query_tracker(dot_torrent: &DotTorrent) -> anyhow::Result<TrackerResponse> {
    let info_hash = dot_torrent.info_hash()?;
    let peer_id = b"00112233445566778899";
    let request = TrackerRequest {
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: dot_torrent.length(),
        compact: 1,
    };
    let url_params =
        serde_urlencoded::to_string(&request).context("urlencode tracker parameters")?;
    let url = format!(
        "{}?{}&info_hash={}&peer_id={}",
        dot_torrent.announce,
        url_params,
        &url_encode(&info_hash),
        &url_encode(&peer_id)
    );
    let response = reqwest::get(url).await.context("query tracker")?;
    let status_is_success = response.status().is_success();
    let response = response.bytes().await.context("fetch tracker response")?;
    println!("{}", String::from_utf8_lossy(&response.to_vec()));
    if status_is_success {
        let response: TrackerResponse =
            serde_bencode::from_bytes(&response).context("parse tracker response")?;
        Ok(response)
    } else {
        let response: TrackerResponseErr =
            serde_bencode::from_bytes(&response).context("parse tracker response")?;
        Err(anyhow!("{}", response.reason))
    }
}

pub fn url_encode(v: &[u8; 20]) -> String {
    // multiply by three because we add a '%' to every byte and
    // every byte converted to hex is two characters
    let mut encoded = String::with_capacity(3 * v.len());
    for &byte in v {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}

#[derive(Debug, Clone)]
pub struct PeerList(pub Vec<SocketAddrV4>);

impl Serialize for PeerList {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut bytes = Vec::with_capacity(6 * self.0.len());
        for peer in &self.0 {
            bytes.extend(peer.ip().octets());
            bytes.extend(peer.port().to_be_bytes());
        }
        serializer.serialize_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for PeerList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(PeerListVisitor)
    }
}

struct PeerListVisitor;

impl<'de> Visitor<'de> for PeerListVisitor {
    type Value = PeerList;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(
            "6 bytes of which 4 bytes are the IP address and last 2 bytes are the port number.",
        )
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: Error,
    {
        if v.len() % 6 != 0 {
            return Err(E::custom(format!("length is {}", v.len())));
        }
        Ok(PeerList(
            v.chunks_exact(6)
                .map(|slice_6| {
                    let ipv4 = Ipv4Addr::new(slice_6[0], slice_6[1], slice_6[2], slice_6[3]);
                    let port = u16::from_be_bytes([slice_6[4], slice_6[5]]);
                    SocketAddrV4::new(ipv4, port)
                })
                .collect(),
        ))
    }
}
