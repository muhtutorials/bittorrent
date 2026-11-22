use crate::BLOCK_SIZE;
use crate::bit_vec::BitVec;
use crate::torrent::TorrentMessage;
use anyhow::Context;
use bytes::{Buf, BufMut, BytesMut};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use kanal::{AsyncReceiver, AsyncSender};
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::net::SocketAddrV4;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio_util::codec::{Decoder, Encoder, Framed};

pub(crate) struct Peer {
    stream: SplitStream<Framed<TcpStream, MessageFramer>>,
    // inflight requests
    inflight: HashMap<u64, OwnedSemaphorePermit>,
    permit_rx: Receiver<(u64, OwnedSemaphorePermit)>,
    peer_tx: Sender<PeerMessage>,
    // chokes
    choke_tx: Sender<ChokeCmd>,
    tor_tx: Sender<TorrentMessage>,
}

impl Peer {
    pub async fn from_addr(
        addr: SocketAddrV4,
        info_hash: [u8; 20],
        tor_tx: Sender<TorrentMessage>,
    ) -> anyhow::Result<Self> {
        let mut conn = TcpStream::connect(addr).await.context("connect to peer")?;
        let mut handshake = Handshake::new(info_hash, *b"00112233445566778899");
        // Safety: Handshake is POD with repr(C)
        let handshake_bytes = handshake.as_bytes_mut();
        conn.write_all(handshake_bytes)
            .await
            .context("write handshake")?;
        conn.read_exact(handshake_bytes)
            .await
            .context("read handshake")?;
        let handshake = Handshake::ref_from_bytes(handshake_bytes);
        anyhow::ensure!(handshake.length == 19);
        anyhow::ensure!(handshake.bittorrent == *b"BitTorrent protocol");
        let (sink, mut stream) = Framed::new(conn, MessageFramer).split();
        let msg = stream
            .next()
            .await
            .expect("peer always sends a bitfield")
            .context("peer message was invalid")?;
        anyhow::ensure!(msg.typ == MessageType::Bitfield);
        let (peer_tx, peer_rx) = channel(32);
        // TODO: what buffer size should be here?
        let (choke_tx, choke_rx) = channel(16);
        let (permit_tx, permit_rx) = channel(32);
        tokio::spawn(async move { send_to_peer(sink, peer_rx, choke_rx, permit_tx) });
        tor_tx.send(TorrentMessage::AddPeer((
            addr,
            BitVec::from_vec(msg.payload),
            peer_tx.clone(),
        )));
        Ok(Self {
            stream,
            inflight: HashMap::new(),
            permit_rx,
            peer_tx,
            choke_tx,
            tor_tx,
        })
    }

    // // shared between both threads
    // let sem = Arc::new(Semaphore::new(32));
    //
    // // channel sink -> stream, carrying the permit
    // let (permit_tx, mut permit_rx) = mpsc::unbounded_channel::<(usize, OwnedSemaphorePermit)>();
    //
    // // ---- sink thread ----
    // async move {
    //     while let Some(block) = block_rx.recv().await {
    //         let permit = sem.clone().acquire_owned().await.unwrap(); // blocks at 32
    //         conn.write_request(block).await;
    //         permit_tx.send((block, permit)).unwrap(); // hand to reader
    //     }
    // }
    //
    // // ---- stream thread ----
    // async move {
    //     let mut in_flight: HashMap<usize, OwnedSemaphorePermit> = HashMap::new();
    //     loop {
    //         tokio::select! {
    //             // permits arrive from the writer
    //             Some((block, permit)) = permit_rx.recv() => {
    //                 in_flight.insert(block, permit);
    //             }
    //             // peer responses arrive
    //             msg = conn.read_message() => match msg {
    //                 Message::Piece { index, data } => {
    //                     if let Some(p) = in_flight.remove(&index) {
    //                         drop(p); // release slot -> sink unblocks
    //                     }
    //                     handle_piece(index, data);
    //                 }
    //                 _ => {}
    //             }
    //         }
    //     }
    // }

    pub(crate) async fn receive(&mut self) -> anyhow::Result<()> {
        loop {
            select! {
                msg = self.permit_rx.recv() => {
                    match msg {
                        Some((block, permit)) => { self.inflight.insert(block, permit); }
                        None => { break; }
                    }
                }
                msg = self.stream.next() => {
                    match msg {
                        Some(msg) => {
                            let Ok(msg) = msg else {
                                break;
                            };
                            match msg.typ {
                                MessageType::Choke => {
                                    self.choke_tx.send(ChokeCmd::Choke);
                                }
                                MessageType::Unchoke => {
                                    self.choke_tx.send(ChokeCmd::Unchoke);
                                }
                                MessageType::Interested
                                | MessageType::NotInterested
                                | MessageType::Request
                                | MessageType::Cancel => {
                                    // not allowing request for now
                                }
                                MessageType::Have => {
                                    // TODO: update bitfield
                                    // TODO: add to list of peers for relevant piece
                                }
                                MessageType::Bitfield => {
                                    anyhow::bail!("peer sent bitfield after handshake")
                                }
                                MessageType::Piece => {
                                    let piece_response = PieceResponse::ref_from_bytes(&msg.payload[..])
                                        .expect("always get all `PieceResponse` fields from peer");
                                    let block_key = to_block_key(piece_response.index(), piece_response.begin());
                                    if self.inflight.remove(&block_key).is_some() {
                                        // TODO: pass the block to cache
                                    }
                                }
                            }
                        }
                        None => { break; }
                    }
                }
            }
        }
        Ok(())
    }

    // pub(crate) async fn participate(
    //     &mut self,
    //     piece_i: usize,
    //     piece_size: usize,
    //     n_blocks: usize,
    //     job_tx: AsyncSender<usize>,
    //     job_rx: AsyncReceiver<usize>,
    //     done_tx: Sender<Message>,
    // ) -> anyhow::Result<()> {
    //     anyhow::ensure!(self.has_piece(piece_i));
    //     self.stream
    //         .send(Message {
    //             typ: MessageType::Interested,
    //             payload: Vec::new(),
    //         })
    //         .await
    //         .context("send interested message")?;
    //     // TODO: timeout, error and return block to submit if next() timed out
    //     'job: loop {
    //         while self.chocked {
    //             let msg = self
    //                 .conn
    //                 .next()
    //                 .await
    //                 .expect("peer always sends an unchoke")
    //                 .context("peer message was invalid")?;
    //             match msg.typ {
    //                 MessageType::Choke => {
    //                     anyhow::bail!("peer sent unchoke while unchoked")
    //                 }
    //                 MessageType::Unchoke => {
    //                     self.chocked = false;
    //                     assert!(msg.payload.is_empty());
    //                     break;
    //                 }
    //                 MessageType::Interested => {
    //                     todo!();
    //                 }
    //                 MessageType::NotInterested => {
    //                     todo!();
    //                 }
    //                 MessageType::Have => {
    //                     // TODO: update bitfield
    //                     // TODO: add to list of peers for relevant piece
    //                     todo!();
    //                 }
    //                 MessageType::Bitfield => {
    //                     anyhow::bail!("peer sent bitfield after handshake")
    //                 }
    //                 MessageType::Request => {
    //                     todo!();
    //                 }
    //                 MessageType::Piece => {
    //                     // piece that we no longer need/are responsible for
    //                     todo!();
    //                 }
    //                 MessageType::Cancel => {
    //                     todo!();
    //                 }
    //             }
    //         }
    //         let Ok(block_i) = job_rx.recv().await else {
    //             break;
    //         };
    //         let block_size = if block_i == n_blocks - 1 {
    //             // calculate last block's size
    //             let modulo = piece_size % BLOCK_SIZE;
    //             if modulo == 0 { BLOCK_SIZE } else { modulo }
    //         } else {
    //             BLOCK_SIZE
    //         };
    //         let mut request = PieceRequest::new(
    //             piece_i as u32,
    //             (block_i * BLOCK_SIZE) as u32,
    //             block_size as u32,
    //         );
    //         let request_bytes = Vec::from(request.as_bytes_mut());
    //         self.conn
    //             .send(Message {
    //                 typ: MessageType::Request,
    //                 payload: request_bytes,
    //             })
    //             .await
    //             .with_context(|| format!("send request for block: {block_i}"))?;
    //         // TODO: timeout and return block to submit if timed out
    //         let mut msg;
    //         loop {
    //             msg = self
    //                 .conn
    //                 .next()
    //                 .await
    //                 .expect("peer always sends an unchoke")
    //                 .context("peer message was invalid")?;
    //             match msg.typ {
    //                 MessageType::Choke => {
    //                     assert!(msg.payload.is_empty());
    //                     self.chocked = true;
    //                     job_tx
    //                         .send(block_i)
    //                         .await
    //                         .expect("we still have a receiver");
    //                     continue 'job;
    //                 }
    //                 MessageType::Unchoke => {
    //                     anyhow::bail!("peer sent unchoke while unchoked")
    //                 }
    //                 MessageType::Interested
    //                 | MessageType::NotInterested
    //                 | MessageType::Request
    //                 | MessageType::Cancel => {
    //                     // not allowing request for now
    //                 }
    //                 MessageType::Have => {
    //                     // TODO: update bitfield
    //                     // TODO: add to list of peers for relevant piece
    //                 }
    //                 MessageType::Bitfield => {
    //                     anyhow::bail!("peer sent bitfield after handshake")
    //                 }
    //                 MessageType::Piece => {
    //                     let piece_response = PieceResponse::ref_from_bytes(&msg.payload[..])
    //                         .expect("always get all `PieceResponse` fields from peer");
    //                     if piece_response.index() as usize != piece_i
    //                         || piece_response.begin() as usize != block_i * BLOCK_SIZE
    //                     {
    //                         // piece that we no longer need/are responsible for
    //                     } else {
    //                         assert_eq!(piece_response.block().len(), block_size);
    //                         break;
    //                     }
    //                 }
    //             }
    //         }
    //         done_tx.send(msg).await
    //             .expect("receiver should not go away while there are active peers (us) and missing blocks (this one)");
    //     }
    //     Ok(())
    // }
}

pub(crate) enum PeerMessage {
    Message(Message),
    Shutdown,
}

enum ChokeCmd {
    Choke,
    Unchoke,
}

// use tokio::sync::{Semaphore, mpsc};
// use std::sync::Arc;
//
// // Limit outstanding requests to 32
// let in_flight = Arc::new(Semaphore::new(32));
//
// loop {
//     // 1. Wait until we have room for another outstanding request
//     let permit = in_flight.clone().acquire_owned().await.unwrap();
//
//     // 2. Now it's safe to pull the next block from the channel
//     let block = match block_rx.recv().await {
//         Some(b) => b,
//         None => break,
//     };
//
//     // 3. Send the request to the remote peer
//     send_request(block).await;
//
//     // 4. Spawn a task to handle the response and release the permit
//     let in_flight_clone = in_flight.clone();
//     tokio::spawn(async move {
//         // Wait for the remote peer to send the block data
//         let data = receive_block_response(block).await;
//
//         // Process the data...
//         handle_block(data);
//
//         // Permit is dropped here, freeing a slot
//         drop(permit);
//     });
// }

// receives messages and sends them to the peer
async fn send_to_peer(
    mut sink: SplitSink<Framed<TcpStream, MessageFramer>, Message>,
    mut rx: Receiver<PeerMessage>,
    mut choke_rx: Receiver<ChokeCmd>,
    permit_tx: Sender<(u64, OwnedSemaphorePermit)>,
) {
    let mut choked = false;
    loop {
        tokio::select! {
            msg = rx.recv(), if !choked => {
                match msg {
                    Some(msg) => {
                        match msg {

                        }
                    }
                    None => break,
                }
            }
            cmd = choke_rx.recv() => {
                match cmd {
                    Some(ChokeCmd::Choke) => choked = true,
                    Some(ChokeCmd::Unchoke) => choked = false,
                    None => break,
                }
            }
        }
    }
}

#[repr(C)]
pub struct Handshake {
    pub length: u8,
    pub bittorrent: [u8; 19],
    pub reserved: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            length: 19,
            bittorrent: *b"BitTorrent protocol",
            reserved: [0; 8],
            info_hash,
            peer_id,
        }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        let bytes = unsafe { self as *mut Self as *mut [u8; size_of::<Self>()] };
        unsafe { &mut *bytes }
    }

    pub fn ref_from_bytes(data: &[u8]) -> &Self {
        let handshake = data as *const [u8] as *const Self;
        unsafe { &*handshake }
    }
}

#[repr(C)]
pub struct PieceRequest {
    // piece index
    index: [u8; 4],
    // offset within the piece
    begin: [u8; 4],
    // requested data length
    length: [u8; 4],
}

impl PieceRequest {
    pub fn new(index: u32, begin: u32, length: u32) -> Self {
        Self {
            index: index.to_be_bytes(),
            begin: begin.to_be_bytes(),
            length: length.to_be_bytes(),
        }
    }

    pub fn index(&self) -> u32 {
        u32::from_be_bytes(self.index)
    }

    pub fn begin(&self) -> u32 {
        u32::from_be_bytes(self.begin)
    }

    pub fn length(&self) -> u32 {
        u32::from_be_bytes(self.length)
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        let bytes = unsafe { self as *mut Self as *mut [u8; size_of::<Self>()] };
        unsafe { &mut *bytes }
    }
}

#[repr(C)]
// NOTE: needs to be (and is)
// #[repr(packed)]
// but can't be marked as such because of the T: ?Sized part
pub struct PieceResponse<T: ?Sized = [u8]> {
    // piece index
    index: [u8; 4],
    // byte offset within the piece
    begin: [u8; 4],
    // block of data, which is a subset
    // of the piece specified by index
    block: T,
}

impl PieceResponse {
    // returns piece index
    pub fn index(&self) -> u32 {
        u32::from_be_bytes(self.index)
    }

    // returns byte offset within the piece
    pub fn begin(&self) -> u32 {
        u32::from_be_bytes(self.begin)
    }

    // returns block data
    pub fn block(&self) -> &[u8] {
        &self.block
    }

    const LEAD: usize = size_of::<PieceResponse<()>>();
    pub fn ref_from_bytes(data: &[u8]) -> Option<&Self> {
        let n = data.len();
        if n < Self::LEAD {
            return None;
        }
        // TODO: why do we need only block length?
        // NOTE: We need the length part of the fat pointer to PieceMessage
        // to hold the length of just the `block` field. And the only way
        // we can change the length of the fat pointer to PieceMessage is by
        // changing the length of the fat pointer to the slice, which we do
        // by slicing it. We can't slice it at the front
        // (as it would invalidate the ptr part of the fat pointer),
        // so we slice it at the back!
        let piece_message = &data[..n - Self::LEAD] as *const [u8] as *const PieceResponse;
        // Safety: PieceMessage is a POD with repr(c) and repr(packed),
        // and the fat pointer data length is the length of the trailing
        // dynamically sized type field (thanks to the LEAD offset).
        Some(unsafe { &*piece_message })
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub typ: MessageType,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MessageType {
    // Tells the peer: "I will not serve you any piece data."
    // You're still connected, but all outstanding requests
    // are implicitly dropped. Usually sent when you have too
    // many uploaders or the peer is leeching without uploading.
    Choke = 0,
    // Tells the peer: "I'm now willing to serve you data."
    // The peer can now send request messages.
    // This is the core of BitTorrent's tit-for-tat incentive:
    // you unchoke peers who upload to you.
    Unchoke = 1,
    // Tells the peer: "I want pieces you have that I don't."
    // Sent after seeing the peer's bitfield/have messages.
    // A peer typically only unchokes you if you've expressed interest.
    Interested = 2,
    // Tells the peer: "I don't need anything you have."
    // Sent when the peer has no pieces you're missing
    // (or you already have everything they offer).
    NotInterested = 3,
    // Tells the peer: "I just completed piece index N."
    // Format: <len=5><id=4><piece index>
    // Used to propagate piece availability without resending
    // the whole bitfield.
    Have = 4,
    // Tells the peer: "Here's every piece I have."
    // A bitmap where bit i equals 1 means you have piece i.
    // Sent immediately after the handshake (if you have any pieces).
    // Only sent once, right after handshake; later changes use have.
    Bitfield = 5,
    // Tells the peer: "Send me a block of piece N."
    // Format: <len=13><id=6><index><begin><length>
    // length is almost always 16 KiB (16384 bytes).
    // You typically pipeline several requests to keep the pipe full.
    Request = 6,
    // The actual data response to a request.
    // Format: <len=9+X><id=7><index><begin><block data>
    // `block data` is the raw bytes for that piece offset.
    // This is the only message carrying actual file content.
    Piece = 7,
    // Tells the peer: "Cancel my earlier request for this block."
    // Format identical to request: <index><begin><length>
    // Sent when you got the block elsewhere (e.g., from another peer)
    // or hit endgame. Helps avoid wasted bandwidth; peers may ignore it.
    // Endgame mode: when a download is almost complete, there's
    // a tendency for the last few pieces to all be downloaded off
    // a single slow connection, taking a very long time. To make
    // sure the last few pieces come in quickly, once requests for all
    // pieces a given downloader doesn't have yet are currently pending,
    // it sends requests for everything to everyone it's downloading from.
    // To keep this from becoming horribly inefficient, it sends cancels
    // to everyone else every time a piece arrives.
    Cancel = 8,
}

impl TryFrom<u8> for MessageType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use MessageType::*;
        match value {
            0 => Ok(Choke),
            1 => Ok(Unchoke),
            2 => Ok(Interested),
            3 => Ok(NotInterested),
            4 => Ok(Have),
            5 => Ok(Bitfield),
            6 => Ok(Request),
            7 => Ok(Piece),
            8 => Ok(Cancel),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid message type")),
        }
    }
}

const MAX: usize = 1 << 16;

// Message format: <length (4 bytes)><type (1 byte)><payload>.
pub struct MessageFramer;

impl Decoder for MessageFramer {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            // Not enough data to read message length.
            return Ok(None);
        }
        // Read message length.
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;
        if length == 0 {
            // This is a keep-alive message which should be discarded.
            src.advance(4);
            // Try again in case buffer has more messages.
            return self.decode(src);
        }
        if src.len() < 5 {
            // Not enough data to read message type.
            return Ok(None);
        }
        // Check that the length is not too large to avoid a DOS
        // attack where the server runs out of memory.
        if length > MAX {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("frame of length {} is too large", length),
            ));
        }
        if src.len() < 4 + length {
            // The full string has not yet arrived.
            //
            // We reserve more space in the buffer. This is not strictly
            // necessary, but is a good idea performance-wise.
            src.reserve(4 + length - src.len());
            // We inform the `Framed` that we need more bytes to form the next
            // frame.
            return Ok(None);
        }
        // Use advance to modify `src` such that it no longer contains
        // this frame.
        let typ = src[4].try_into()?;
        // First byte is the message type.
        let payload = if length > 1 {
            src[5..4 + length].to_vec()
        } else {
            Vec::new()
        };
        src.advance(4 + length);
        Ok(Some(Message { typ, payload }))
    }
}

impl Encoder<Message> for MessageFramer {
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Don't send a message if it is longer than
        // the other end will accept.
        // "+1" is the message type.
        if item.payload.len() + 1 > MAX {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("frame of length {} is too large", item.payload.len() + 1),
            ));
        }
        // Convert the length into a byte array.
        let length_bytes = u32::to_be_bytes(item.payload.len() as u32 + 1);
        // Reserve space in the buffer.
        dst.reserve(4 + 1 + item.payload.len());
        // Write the length, tag and string to the buffer.
        dst.extend_from_slice(&length_bytes);
        dst.put_u8(item.typ as u8);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}

// packs piece index and block index into a single `u64`
fn to_block_key(piece_i: u32, block_i: u32) -> u64 {
    ((piece_i as u64) << 32) | (block_i as u64 & 0xFFFF_FFFF)
}

// unpacks piece index and block index from a single `u64`
fn from_block_key(key: u64) -> (u32, u32) {
    ((key >> 32) as u32, key as u32)
}
