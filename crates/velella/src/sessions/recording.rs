//! Sidecar geometry journal. Raw terminal.bin is never rewritten.
use anyhow::Result;
use serde::Serialize;
use std::{fs::{File, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, path::Path};

const RECORD: u64 = 16;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Geometry { pub offset:u64, pub rows:u16, pub cols:u16, pub reset:bool, pub sequence:u64 }

pub struct Recording {
    log:File,
    geometry:Option<File>,
    offset:u64,
    last:Option<Geometry>,
}
impl Recording {
    pub fn new(log:File, geometry:Option<File>)->Result<Self> {
        let offset=log.metadata()?.len();
        let mut recording=Self{log,geometry,offset,last:None};
        recording.resize(24,80,true)?;
        Ok(recording)
    }
    pub fn open(directory:&Path)->Result<Self> {
        let log=OpenOptions::new().create(true).append(true).open(directory.join("terminal.bin"))?;
        let geometry=OpenOptions::new().create(true).append(true).open(directory.join("terminal.geometry"))?;
        let length=geometry.metadata()?.len();
        if length%RECORD!=0 {
            // Owner lock is held by host::run. Preserve every byte of the
            // damaged journal before removing only its incomplete final entry.
            let backup=directory.join(format!("terminal.geometry.incomplete-{}",uuid::Uuid::now_v7()));
            let mut source=File::open(directory.join("terminal.geometry"))?;
            let mut copy=OpenOptions::new().create_new(true).write(true).open(backup)?;
            std::io::copy(&mut source,&mut copy)?;copy.sync_all()?;
            // Windows append handles lack the truncate access right.
            OpenOptions::new().write(true).open(directory.join("terminal.geometry"))?.set_len(length-length%RECORD)?;
        }
        Self::new(log,Some(geometry))
    }
    pub fn write(&mut self,bytes:&[u8])->Result<()> {
        self.log.write_all(bytes)?;self.log.flush()?;
        self.offset+=bytes.len() as u64;Ok(())
    }
    pub fn resize(&mut self,rows:u16,cols:u16,reset:bool)->Result<()> {
        if !reset && self.last.is_some_and(|last|last.rows==rows&&last.cols==cols){return Ok(())}
        // Several resizes can occur without output. Preserve a new-process
        // reset at that same offset when readers select the latest geometry.
        let reset=reset||self.last.is_some_and(|last|last.offset==self.offset&&last.reset);
        let event=Geometry{offset:self.offset,rows,cols,reset,sequence:0};
        if let Some(file)=&mut self.geometry {
            let mut bytes=[0u8;16];bytes[..8].copy_from_slice(&event.offset.to_le_bytes());
            bytes[8..10].copy_from_slice(&rows.to_le_bytes());bytes[10..12].copy_from_slice(&cols.to_le_bytes());
            bytes[12..16].copy_from_slice(&u32::from(reset).to_le_bytes());
            file.write_all(&bytes)?;file.flush()?;
        }
        self.last=Some(event);Ok(())
    }
}

fn event_at(file:&mut File,index:u64)->Result<Geometry>{
    file.seek(SeekFrom::Start(index*RECORD))?;let mut bytes=[0;16];file.read_exact(&mut bytes)?;
    let rows=u16::from_le_bytes(bytes[8..10].try_into()?);let cols=u16::from_le_bytes(bytes[10..12].try_into()?);
    let flags=u32::from_le_bytes(bytes[12..16].try_into()?);
    anyhow::ensure!((1..=1000).contains(&rows)&&(1..=1000).contains(&cols)&&flags<=1,"invalid geometry journal");
    Ok(Geometry{offset:u64::from_le_bytes(bytes[..8].try_into()?),rows,cols,reset:flags==1,sequence:index})
}

/// O(log n + page events): never scan an eight-hour journal on every poll.
/// A partial final record is ignored; it is not committed until all 16 bytes exist.
pub fn page(directory:&Path,from:u64,end:u64)->Result<Vec<Geometry>> {
    let mut file=match File::open(directory.join("terminal.geometry")) {
        Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(vec![]),Err(e)=>return Err(e.into()),
    };
    let count=file.metadata()?.len()/RECORD;
    let (mut low,mut high)=(0,count);
    while low<high {let mid=low+(high-low)/2;if event_at(&mut file,mid)?.offset<=from{low=mid+1}else{high=mid}}
    let mut events=Vec::new();
    if low>0 {let mut event=event_at(&mut file,low-1)?;if event.offset<from{event.reset=false}events.push(event)}
    for index in low..count {
        let event=event_at(&mut file,index)?;if event.offset>end{break}
        anyhow::ensure!(events.len()<65536,"too many geometry changes in one output page");
        anyhow::ensure!(events.last().is_none_or(|last|last.offset<=event.offset),"unordered geometry journal");
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn geometry_tracks_offsets_pages_and_restart_without_changing_raw_log(){
        let directory=std::env::temp_dir().join(format!("velella-geometry-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        assert!(page(&directory,0,100).unwrap().is_empty());
        let mut log=Recording::open(&directory).unwrap();
        log.resize(30,100,false).unwrap();log.write(b"abc").unwrap();
        log.resize(40,120,false).unwrap();log.write(b"def").unwrap();drop(log);
        let first=page(&directory,0,3).unwrap();
        assert_eq!((first[0].offset,first[0].cols,first[0].reset),(0,100,true));
        assert_eq!((first[1].offset,first[1].cols),(3,120));
        assert!(!page(&directory,4,6).unwrap()[0].reset);
        let mut restart=Recording::open(&directory).unwrap();restart.write(b"ghi").unwrap();drop(restart);
        let tail=page(&directory,6,9).unwrap();assert!(tail[0].reset);assert_eq!(tail[0].cols,80);
        assert_eq!(std::fs::read(directory.join("terminal.bin")).unwrap(),b"abcdefghi");
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test] fn legacy_prefix_and_partial_record_do_not_invent_geometry(){
        let directory=std::env::temp_dir().join(format!("velella-geometry-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();std::fs::write(directory.join("terminal.bin"),b"legacy").unwrap();
        drop(Recording::open(&directory).unwrap());
        assert!(page(&directory,0,5).unwrap().is_empty());
        assert_eq!(page(&directory,6,6).unwrap()[0].offset,6);
        OpenOptions::new().append(true).open(directory.join("terminal.geometry")).unwrap().write_all(b"partial").unwrap();
        assert_eq!(page(&directory,6,6).unwrap().len(),1);
        drop(Recording::open(&directory).unwrap());
        let backup=std::fs::read_dir(&directory).unwrap().filter_map(Result::ok).find(|entry|entry.file_name().to_string_lossy().contains("incomplete-")).unwrap();
        assert!(std::fs::read(backup.path()).unwrap().ends_with(b"partial"));
        assert_eq!(page(&directory,6,6).unwrap()[0].sequence,1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
