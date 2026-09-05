use std::io::{Read, Seek, SeekFrom, Write};

use anyhow::{bail, Context, Result};

use crate::domain::SessionPayload;

const MAGIC: &[u8; 8] = b"NHSEGREC";
const VERSION: u16 = 1;
const HEADER_LENGTH: u16 = 52;

#[derive(Clone, Copy, Debug)]
pub struct RecordLocation {
    pub offset: u64,
    pub length: u64,
}

pub fn append_record<W>(
    writer: &mut W,
    session_id: i64,
    c2s: &[u8],
    s2c: &[u8],
) -> Result<RecordLocation>
where
    W: Write + Seek,
{
    let offset = writer.seek(SeekFrom::End(0)).context("seek segment end")?;
    let payload_length = c2s
        .len()
        .checked_add(s2c.len())
        .context("payload length overflow")?;
    let record_length = usize::from(HEADER_LENGTH)
        .checked_add(payload_length)
        .context("record length overflow")? as u64;

    let mut checksum = crc32fast::Hasher::new();
    checksum.update(c2s);
    checksum.update(s2c);

    let mut header = Vec::with_capacity(usize::from(HEADER_LENGTH));
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&VERSION.to_le_bytes());
    header.extend_from_slice(&HEADER_LENGTH.to_le_bytes());
    header.extend_from_slice(&record_length.to_le_bytes());
    header.extend_from_slice(&session_id.to_le_bytes());
    header.extend_from_slice(&(c2s.len() as u64).to_le_bytes());
    header.extend_from_slice(&(s2c.len() as u64).to_le_bytes());
    header.extend_from_slice(&checksum.finalize().to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(header.len(), usize::from(HEADER_LENGTH));

    writer.write_all(&header).context("write segment header")?;
    writer.write_all(c2s).context("write c2s payload")?;
    writer.write_all(s2c).context("write s2c payload")?;

    Ok(RecordLocation {
        offset,
        length: record_length,
    })
}

pub fn read_record<R>(
    reader: &mut R,
    expected_session_id: i64,
    location: RecordLocation,
) -> Result<SessionPayload>
where
    R: Read + Seek,
{
    reader
        .seek(SeekFrom::Start(location.offset))
        .context("seek segment record")?;
    let mut header = [0u8; HEADER_LENGTH as usize];
    reader
        .read_exact(&mut header)
        .context("read segment header")?;

    if &header[0..8] != MAGIC {
        bail!("invalid segment record magic");
    }
    let version = u16::from_le_bytes(header[8..10].try_into().unwrap());
    if version != VERSION {
        bail!("unsupported segment record version {version}");
    }
    let header_length = u16::from_le_bytes(header[10..12].try_into().unwrap());
    if header_length != HEADER_LENGTH {
        bail!("unsupported segment header length {header_length}");
    }
    let record_length = u64::from_le_bytes(header[12..20].try_into().unwrap());
    if record_length != location.length {
        bail!("segment record length does not match SQLite metadata");
    }
    let session_id = i64::from_le_bytes(header[20..28].try_into().unwrap());
    if session_id != expected_session_id {
        bail!("segment record belongs to session {session_id}");
    }

    let c2s_length = u64::from_le_bytes(header[28..36].try_into().unwrap());
    let s2c_length = u64::from_le_bytes(header[36..44].try_into().unwrap());
    let expected_payload_length = record_length
        .checked_sub(u64::from(HEADER_LENGTH))
        .context("invalid record length")?;
    if c2s_length.checked_add(s2c_length) != Some(expected_payload_length) {
        bail!("invalid stream lengths in segment record");
    }

    let payload_length: usize = expected_payload_length
        .try_into()
        .context("segment record is too large for this platform")?;
    let mut payload = vec![0u8; payload_length];
    reader
        .read_exact(&mut payload)
        .context("read segment payload")?;

    let expected_checksum = u32::from_le_bytes(header[44..48].try_into().unwrap());
    if crc32fast::hash(&payload) != expected_checksum {
        bail!("segment payload checksum mismatch");
    }
    let split: usize = c2s_length.try_into().context("c2s stream too large")?;
    let s2c = payload.split_off(split);
    Ok(SessionPayload { c2s: payload, s2c })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{append_record, read_record};

    #[test]
    fn round_trips_record() {
        let mut data = Cursor::new(Vec::new());
        let location = append_record(&mut data, 42, b"request", b"response").unwrap();
        let payload = read_record(&mut data, 42, location).unwrap();
        assert_eq!(payload.c2s, b"request");
        assert_eq!(payload.s2c, b"response");
    }

    #[test]
    fn detects_payload_corruption() {
        let mut data = Cursor::new(Vec::new());
        let location = append_record(&mut data, 42, b"request", b"response").unwrap();
        data.get_mut()[location.offset as usize + 52] ^= 0xff;
        assert!(read_record(&mut data, 42, location).is_err());
    }
}
