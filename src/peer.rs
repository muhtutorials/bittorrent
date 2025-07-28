use crate::BLOCK_MAX;
use crate::bitfield::Bitfield;
use anyhow::Context;
use bytes::{Buf, BufMut, BytesMut};
use futures_util::{SinkExt, StreamExt};
use kanal::{AsyncReceiver, AsyncSender};
use std::io::{Error, ErrorKind};
use std::net::SocketAddrV4;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::Sender;
use tokio_util::codec::{Decoder, Encoder, Framed};

// so that we can respond from request from other side, also choking and unchoking other side
pub(crate) struct Peer {
    // addr: SocketAddrV4,
    stream: Framed<TcpStream, MessageFramer>,
    bitfield: Bitfield,
    chocked: bool,
}

impl Peer {
    pub async fn new(addr: SocketAddrV4, info_hash: [u8; 20]) -> anyhow::Result<Self> {
        let mut stream = TcpStream::connect(addr).await.context("connect to peer")?;
        let mut handshake = Handshake::new(info_hash, *b"00112233445566778899");
        // TODO: remove unsafe and implement serde instead
        // drop handshake_bytes
        // Safety: Handshake is POD with repr(C)
        let handshake_bytes = handshake.as_bytes_mut();
        stream
            .write_all(handshake_bytes)
            .await
            .context("write handshake")?;
        stream
            .read_exact(handshake_bytes)
            .await
            .context("read handshake")?;
        let handshake = Handshake::ref_from_bytes(handshake_bytes);
        anyhow::ensure!(handshake.length == 19);
        anyhow::ensure!(handshake.bittorrent == *b"BitTorrent protocol");
        let mut stream = Framed::new(stream, MessageFramer);
        let msg = stream
            .next()
            .await
            .expect("peer always sends a bitfield")
            .context("peer message was invalid")?;
        anyhow::ensure!(msg.typ == MessageType::Bitfield);
        Ok(Self {
            stream,
            bitfield: Bitfield::from_payload(msg.payload),
            chocked: true,
        })
    }

    pub(crate) fn has_piece(&self, piece_i: usize) -> bool {
        self.bitfield.has_piece(piece_i)
    }

    pub(crate) async fn participate(
        &mut self,
        piece_i: usize,
        piece_size: usize,
        n_blocks: usize,
        job_tx: AsyncSender<usize>,
        job_rx: AsyncReceiver<usize>,
        done_tx: Sender<Message>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.has_piece(piece_i));
        self.stream
            .send(Message {
                typ: MessageType::Interested,
                payload: Vec::new(),
            })
            .await
            .context("send interested message")?;

        // TODO: timeout, error and return block to submit if next() timed out
        'job: loop {
            while self.chocked {
                let msg = self
                    .stream
                    .next()
                    .await
                    .expect("peer always sends an unchoke")
                    .context("peer message was invalid")?;
                match msg.typ {
                    MessageType::Choke => {
                        anyhow::bail!("peer sent unchoke while unchoked")
                    }
                    MessageType::Unchoke => {
                        self.chocked = false;
                        assert!(msg.payload.is_empty());
                        break;
                    }
                    MessageType::Interested
                    | MessageType::NotInterested
                    | MessageType::Request
                    | MessageType::Cancel => {
                        // not allowing requests for now
                    }
                    MessageType::Have => {
                        // TODO: update bitfield
                        // TODO: add to list of peers for relevant piece
                    }
                    MessageType::Bitfield => {
                        anyhow::bail!("peer sent bitfield after handshake")
                    }
                    MessageType::Piece => {
                        // piece that we no longer need/are responsible for
                    }
                }
            }

            let Ok(block_i) = job_rx.recv().await else {
                break;
            };

            let block_size = if block_i == n_blocks - 1 {
                // calculate last block's size
                let modulo = piece_size % BLOCK_MAX;
                if modulo == 0 { BLOCK_MAX } else { modulo }
            } else {
                BLOCK_MAX
            };
            let mut request = PieceRequest::new(
                piece_i as u32,
                (block_i * BLOCK_MAX) as u32,
                block_size as u32,
            );
            let request_bytes = Vec::from(request.as_bytes_mut());
            self.stream
                .send(Message {
                    typ: MessageType::Request,
                    payload: request_bytes,
                })
                .await
                .with_context(|| format!("send request for block: {block_i}"))?;
            // TODO: timeout and return block to submit if timed out
            let mut msg;
            loop {
                msg = self
                    .stream
                    .next()
                    .await
                    .expect("peer always sends an unchoke")
                    .context("peer message was invalid")?;
                match msg.typ {
                    MessageType::Choke => {
                        assert!(msg.payload.is_empty());
                        self.chocked = true;
                        job_tx
                            .send(block_i)
                            .await
                            .expect("we still have a receiver");
                        continue 'job;
                    }
                    MessageType::Unchoke => {
                        anyhow::bail!("peer sent unchoke while unchoked")
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
                        if piece_response.index() as usize != piece_i
                            || piece_response.begin() as usize != block_i * BLOCK_MAX
                        {
                            // piece that we no longer need/are responsible for
                        } else {
                            assert_eq!(piece_response.block().len(), block_size);
                            break;
                        }
                    }
                }
            }
            done_tx.send(msg).await
                .expect("receiver should not go away while there are active peers (us) and missing blocks (this one)");
        }
        Ok(())
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
    pub fn index(&self) -> u32 {
        u32::from_be_bytes(self.index)
    }

    pub fn begin(&self) -> u32 {
        u32::from_be_bytes(self.begin)
    }

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
    Choke = 0,
    // permission to download
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
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

// Message form: <length prefix><message ID><payload>.
pub struct MessageFramer;

const MAX: usize = 1 << 16;

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
        let length_slice = u32::to_be_bytes(item.payload.len() as u32 + 1);

        // Reserve space in the buffer.
        dst.reserve(4 + 1 + item.payload.len());

        // Write the length, tag and string to the buffer.
        dst.extend_from_slice(&length_slice);
        dst.put_u8(item.typ as u8);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}
