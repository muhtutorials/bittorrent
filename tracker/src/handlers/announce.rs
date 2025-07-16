use crate::error::ErrResp;
use crate::state::AppState;
use crate::utils::percent_decode;
use anyhow::anyhow;
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::StatusCode;
use serde::Serialize;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use std::net::SocketAddr;

pub async fn get(
    RawQuery(query): RawQuery,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<(StatusCode, Vec<u8>), ErrResp> {
    let query = query.ok_or(ErrResp::bad_request(anyhow!("invalid URL query string")))?;
    let params = parse_query(&query).map_err(|e| {
        ErrResp::bad_request(anyhow!(e))
    })?;
    println!("{:?}", params);
    let peer_addr = SocketAddr::new(addr.ip(), params.port);
    let mut torrents = state.torrents.lock().expect("mutex was poisoned");
    let mut peers = Vec::new();
    match torrents.items.entry(params.info_hash) {
        Entry::Vacant(entry) => {
            let mut available_peers = VecDeque::new();
            available_peers.push_back(peer_addr);
            entry.insert(available_peers);
            peers.push(peer_addr)
        }
        Entry::Occupied(mut entry) => {
            let available_peers = entry.get_mut();
            if let Some(index) = available_peers.iter().position(|&addr| addr == peer_addr) {
                available_peers.remove(index);
                available_peers.push_back(peer_addr);
                peers.extend(available_peers.iter())
            }
        }
    };
    let peer_resp = PeersResp { peers };
    let peer_resp =
        serde_bencode::to_bytes(&peer_resp).map_err(|e| ErrResp::server_error(anyhow!(e)))?;
    Ok((StatusCode::OK, peer_resp))
}

#[derive(Debug)]
pub struct AnnounceParams {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub compact: u8,
}

fn parse_query(s: &str) -> anyhow::Result<AnnounceParams> {
    let mut info_hash: Option<[u8; 20]> = None;
    let mut peer_id: Option<[u8; 20]> = None;
    let mut port = None;
    let mut uploaded = None;
    let mut downloaded = None;
    let mut left = None;
    let mut compact = None;
    for pair in s.split('&') {
        let mut parts = pair.split('=');
        let key = parts.next().ok_or(anyhow!("missing query key"))?;
        let value = parts.next().ok_or(anyhow!("missing query value"))?;
        match key {
            "info_hash" => {
                let dec = percent_decode(value.as_bytes());
                info_hash = Some(dec.collect::<Vec<u8>>().try_into()
                    .map_err(|_| anyhow!("invalid query parameter `info_hash`"))?)
            }
            "peer_id" => {
                let dec = percent_decode(value.as_bytes());
                peer_id = Some(dec.collect::<Vec<u8>>().try_into()
                    .map_err(|_| anyhow!("invalid query parameter `peer_id`"))?)
            }
            "port" => {
                port = Some(
                    value
                        .parse()
                        .map_err(|_| anyhow!("invalid query parameter `port`"))?,
                )
            }
            "uploaded" => {
                uploaded = Some(
                    value
                        .parse()
                        .map_err(|_| anyhow!("invalid query parameter `uploaded`"))?,
                )
            }
            "downloaded" => {
                downloaded = Some(
                    value
                        .parse()
                        .map_err(|_| anyhow!("invalid query parameter `downloaded`"))?,
                )
            }
            "left" => {
                left = Some(
                    value
                        .parse()
                        .map_err(|_| anyhow!("invalid query parameter `left`"))?,
                )
            }
            "compact" => {
                compact = Some(
                    value
                        .parse()
                        .map_err(|_| anyhow!("invalid query parameter `compact`"))?,
                )
            }
            _ => return Err(anyhow!("Unknown parameter: {key}")),
        }
    }
    Ok(AnnounceParams {
        info_hash: info_hash.ok_or(anyhow!("missing query parameter `info_hash`"))?,
        peer_id: peer_id.ok_or(anyhow!("missing query parameter `peer_id`"))?,
        port: port.ok_or(anyhow!("missing query parameter `port`"))?,
        uploaded: uploaded.ok_or(anyhow!("missing query parameter `uploaded`"))?,
        downloaded: downloaded.ok_or(anyhow!("missing query parameter `downloaded`"))?,
        left: left.ok_or(anyhow!("missing query parameter `left`"))?,
        compact: compact.ok_or(anyhow!("missing query parameter `compact`"))?,
    })
}

#[derive(Serialize)]
pub struct PeersResp {
    peers: Vec<SocketAddr>,
}
