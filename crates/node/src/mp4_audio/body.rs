use super::*;
use crate::files::OpenedFile;
use bytes::Bytes;
use std::{
    io::{self, SeekFrom},
    ops::Range,
    sync::Arc,
};
use tokio::io::{AsyncRead, AsyncSeek};

pub(super) struct Extent {
    pub(super) offset: u64,
    pub(super) length: usize,
}

pub(super) struct Batch {
    pub(super) offset: u64,
    pub(super) length: usize,
    pub(super) slices: Vec<Range<usize>>,
}

pub(super) struct VirtualBody {
    pub(super) file: tokio::fs::File,
    pub(super) prefix: Bytes,
    pub(super) batches: Vec<Batch>,
    pub(super) batch: usize,
    pub(super) seeking: bool,
    pub(super) reading: bool,
    pub(super) filled: usize,
    pub(super) buffer: Vec<u8>,
    pub(super) remaining: u64,
    pub(super) _lease: Option<Arc<crate::cache_dirs::CacheDirectory>>,
}

impl VirtualBody {
    pub(super) fn new(
        mut opened: OpenedFile,
        window: Window,
        start: u64,
        length: u64,
    ) -> io::Result<Self> {
        let end = start + length;
        let prefix_end = (window.prefix.len() as u64).min(end);
        let prefix = if start < prefix_end {
            window.prefix.slice(start as usize..prefix_end as usize)
        } else {
            Bytes::new()
        };
        let mut position = window.prefix.len() as u64;
        let mut extents: Vec<Extent> = Vec::new();
        for sample in window.samples {
            let next = position + u64::from(sample.size);
            let left = start.max(position);
            let right = end.min(next);
            if left < right {
                let offset = sample.offset + left - position;
                let length = (right - left) as usize;
                if let Some(last) = extents
                    .last_mut()
                    .filter(|last| last.offset + last.length as u64 == offset)
                {
                    last.length += length;
                } else {
                    extents.push(Extent { offset, length });
                }
            }
            position = next;
        }
        let mut batches: Vec<Batch> = Vec::new();
        for extent in extents {
            let mut offset = extent.offset;
            let mut remaining = extent.length;
            while remaining > 0 {
                let take = remaining.min(BLOCK);
                if let Some(last) = batches.last_mut().filter(|last| {
                    offset >= last.offset + last.length as u64
                        && offset + take as u64 - last.offset <= BLOCK as u64
                }) {
                    let begin = (offset - last.offset) as usize;
                    last.slices.push(begin..begin + take);
                    last.length = begin + take;
                } else {
                    batches.push(Batch {
                        offset,
                        length: take,
                        slices: std::iter::once(0..take).collect(),
                    });
                }
                offset += take as u64;
                remaining -= take;
            }
        }
        let lease = opened.take_cache_lease();
        Ok(Self {
            file: opened.into_tokio_file(),
            prefix,
            batches,
            batch: 0,
            seeking: false,
            reading: false,
            filled: 0,
            buffer: vec![0; BLOCK],
            remaining: length,
            _lease: lease,
        })
    }
}

impl http_body::Body for VirtualBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<io::Result<http_body::Frame<Bytes>>>> {
        use std::{pin::Pin, task::Poll};
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        if !self.prefix.is_empty() {
            let take = self.prefix.len().min(BLOCK);
            let data = self.prefix.split_to(take);
            self.remaining -= data.len() as u64;
            return Poll::Ready(Some(Ok(http_body::Frame::data(data))));
        }
        let this = &mut *self;
        let Some(batch) = this.batches.get(this.batch) else {
            this.remaining = 0;
            return Poll::Ready(Some(Err(invalid("native audio virtual body truncated"))));
        };
        if !this.seeking && !this.reading {
            if let Err(error) = Pin::new(&mut this.file).start_seek(SeekFrom::Start(batch.offset)) {
                this.remaining = 0;
                return Poll::Ready(Some(Err(error)));
            }
            this.seeking = true;
        }
        if this.seeking {
            match Pin::new(&mut this.file).poll_complete(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    this.remaining = 0;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(Ok(_)) => {
                    this.seeking = false;
                    this.reading = true;
                }
            }
        }
        while this.filled < batch.length {
            let mut read = tokio::io::ReadBuf::new(&mut this.buffer[this.filled..batch.length]);
            match Pin::new(&mut this.file).poll_read(cx, &mut read) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    this.remaining = 0;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(Ok(())) => {
                    let n = read.filled().len();
                    if n == 0 {
                        this.remaining = 0;
                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "source truncated during native audio stream",
                        ))));
                    }
                    this.filled += n;
                }
            }
        }
        let total = batch.slices.iter().map(|slice| slice.len()).sum();
        let mut output = Vec::with_capacity(total);
        for slice in &batch.slices {
            output.extend_from_slice(&this.buffer[slice.clone()]);
        }
        this.batch += 1;
        this.filled = 0;
        this.reading = false;
        this.remaining -= output.len() as u64;
        Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from(output)))))
    }
    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }
    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.remaining)
    }
}
