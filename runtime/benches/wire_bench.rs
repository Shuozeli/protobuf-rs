//! Benchmarks for wire format encoding/decoding and message round-trips.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use protoc_rs_runtime::unknown_fields::{self, UnknownFields};
use protoc_rs_runtime::wire::{self, WireType};
use protoc_rs_runtime::*;

// ---------------------------------------------------------------------------
// A benchmark message with multiple field types
// ---------------------------------------------------------------------------

#[derive(Clone, Default, PartialEq, Debug)]
struct BenchMsg {
    pub id: i64,
    pub name: String,
    pub score: f64,
    pub tags: Vec<String>,
    pub data: Vec<u8>,
    pub _cached_size: CachedSize,
    pub _unknown_fields: UnknownFields,
}

impl Message for BenchMsg {
    fn compute_size(&self) -> u32 {
        let mut size = 0u32;
        if self.id != 0 {
            size += (1 + wire::varint_len(self.id as u64)) as u32;
        }
        if !self.name.is_empty() {
            size += (1 + wire::varint_len(self.name.len() as u64) + self.name.len()) as u32;
        }
        if self.score != 0.0 {
            size += 1 + 8;
        }
        for t in &self.tags {
            size += (1 + wire::varint_len(t.len() as u64) + t.len()) as u32;
        }
        if !self.data.is_empty() {
            size += (1 + wire::varint_len(self.data.len() as u64) + self.data.len()) as u32;
        }
        size += self._unknown_fields.encoded_len() as u32;
        self._cached_size.set(size);
        size
    }

    fn write_to(&self, buf: &mut Vec<u8>) {
        if self.id != 0 {
            wire::encode_tag(1, WireType::Varint, buf);
            wire::encode_varint(self.id as u64, buf);
        }
        if !self.name.is_empty() {
            wire::encode_tag(2, WireType::LengthDelimited, buf);
            wire::encode_varint(self.name.len() as u64, buf);
            buf.extend_from_slice(self.name.as_bytes());
        }
        if self.score != 0.0 {
            wire::encode_tag(3, WireType::Fixed64, buf);
            wire::encode_fixed64(self.score.to_bits(), buf);
        }
        for t in &self.tags {
            wire::encode_tag(4, WireType::LengthDelimited, buf);
            wire::encode_varint(t.len() as u64, buf);
            buf.extend_from_slice(t.as_bytes());
        }
        if !self.data.is_empty() {
            wire::encode_tag(5, WireType::LengthDelimited, buf);
            wire::encode_varint(self.data.len() as u64, buf);
            buf.extend_from_slice(&self.data);
        }
        self._unknown_fields.write_to(buf);
    }

    fn merge_field(
        &mut self,
        field_number: u32,
        wire_type: WireType,
        buf: &[u8],
        options: &DecodeOptions,
    ) -> Result<usize, DecodeError> {
        match field_number {
            1 => {
                let (v, c) = wire::decode_varint(buf)?;
                self.id = v as i64;
                Ok(c)
            }
            2 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.name = std::str::from_utf8(&buf[h..end])
                    .map_err(|_| DecodeError::InvalidUtf8)?
                    .to_string();
                Ok(end)
            }
            3 => {
                let (v, c) = wire::decode_fixed64(buf)?;
                self.score = f64::from_bits(v);
                Ok(c)
            }
            4 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.tags.push(
                    std::str::from_utf8(&buf[h..end])
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string(),
                );
                Ok(end)
            }
            5 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.data = buf[h..end].to_vec();
                Ok(end)
            }
            _ => {
                let (f, c) = unknown_fields::decode_unknown_field(
                    buf,
                    field_number,
                    wire_type,
                    options.recursion_limit,
                )?;
                self._unknown_fields.push(f);
                Ok(c)
            }
        }
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
    fn cached_size(&self) -> &CachedSize {
        &self._cached_size
    }
    fn unknown_fields(&self) -> &UnknownFields {
        &self._unknown_fields
    }
    fn unknown_fields_mut(&mut self) -> &mut UnknownFields {
        &mut self._unknown_fields
    }
    fn full_name() -> &'static str {
        "bench.BenchMsg"
    }
}

// A corresponding view type
#[derive(Default, Debug, PartialEq)]
struct BenchMsgView<'a> {
    pub id: i64,
    pub name: &'a str,
    pub score: f64,
    pub tags: Vec<&'a str>,
    pub data: &'a [u8],
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> protoc_rs_runtime::view::MessageView<'a> for BenchMsgView<'a> {
    fn merge_field_view(
        &mut self,
        field_number: u32,
        wire_type: WireType,
        buf: &'a [u8],
        recursion_limit: u32,
    ) -> Result<usize, DecodeError> {
        match field_number {
            1 => {
                let (v, c) = wire::decode_varint(buf)?;
                self.id = v as i64;
                Ok(c)
            }
            2 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.name =
                    std::str::from_utf8(&buf[h..end]).map_err(|_| DecodeError::InvalidUtf8)?;
                Ok(end)
            }
            3 => {
                let (v, c) = wire::decode_fixed64(buf)?;
                self.score = f64::from_bits(v);
                Ok(c)
            }
            4 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.tags
                    .push(std::str::from_utf8(&buf[h..end]).map_err(|_| DecodeError::InvalidUtf8)?);
                Ok(end)
            }
            5 => {
                let (len, h) = wire::decode_varint(buf)?;
                let end = h + len as usize;
                if end > buf.len() {
                    return Err(DecodeError::UnexpectedEof);
                }
                self.data = &buf[h..end];
                Ok(end)
            }
            _ => wire::skip_field(buf, wire_type, recursion_limit),
        }
    }
}

fn make_bench_msg() -> BenchMsg {
    BenchMsg {
        id: 12345678,
        name: "benchmark test message with a reasonably long name".to_string(),
        score: 3.14159265358979,
        tags: vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "tag3".to_string(),
            "longer-tag-value".to_string(),
        ],
        data: vec![0u8; 256],
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_varint(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint");

    group.bench_function("encode_small", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(10);
            wire::encode_varint(black_box(42), &mut buf);
            black_box(buf);
        })
    });

    group.bench_function("encode_large", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(10);
            wire::encode_varint(black_box(u64::MAX), &mut buf);
            black_box(buf);
        })
    });

    let small_buf = {
        let mut b = Vec::new();
        wire::encode_varint(42, &mut b);
        b
    };
    group.bench_function("decode_small", |b| {
        b.iter(|| {
            let (v, _) = wire::decode_varint(black_box(&small_buf)).unwrap();
            black_box(v);
        })
    });

    let large_buf = {
        let mut b = Vec::new();
        wire::encode_varint(u64::MAX, &mut b);
        b
    };
    group.bench_function("decode_large", |b| {
        b.iter(|| {
            let (v, _) = wire::decode_varint(black_box(&large_buf)).unwrap();
            black_box(v);
        })
    });

    group.finish();
}

fn bench_message(c: &mut Criterion) {
    let msg = make_bench_msg();
    let encoded = encode(&msg);

    let mut group = c.benchmark_group("message");

    group.bench_function("encode", |b| {
        b.iter(|| {
            let bytes = encode(black_box(&msg));
            black_box(bytes);
        })
    });

    group.bench_function("decode_owned", |b| {
        b.iter(|| {
            let m: BenchMsg = decode(black_box(&encoded)).unwrap();
            black_box(m);
        })
    });

    group.bench_function("decode_view", |b| {
        b.iter(|| {
            let v: BenchMsgView = decode_view(black_box(&encoded)).unwrap();
            black_box(v);
        })
    });

    group.bench_function("round_trip", |b| {
        b.iter(|| {
            let bytes = encode(black_box(&msg));
            let m: BenchMsg = decode(&bytes).unwrap();
            black_box(m);
        })
    });

    group.finish();
}

fn bench_compute_size(c: &mut Criterion) {
    let msg = make_bench_msg();

    c.bench_function("compute_size", |b| {
        b.iter(|| {
            let size = black_box(&msg).compute_size();
            black_box(size);
        })
    });
}

criterion_group!(benches, bench_varint, bench_message, bench_compute_size);
criterion_main!(benches);
