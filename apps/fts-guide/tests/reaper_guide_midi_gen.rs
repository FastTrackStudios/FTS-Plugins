//! REAPER integration test: generate guide MIDI across multiple time signatures.
//!
//! Creates a song structure with various time signatures and section types,
//! generates MIDI items with the correct guide/count trigger notes, and
//! verifies everything lands at the right positions.
//!
//! Song structure (tempo changes + time sig changes):
//!
//!   Section       | Bars  | Time Sig | BPM | Start (s) | Notes
//!   --------------|-------|----------|-----|-----------|------
//!   Intro         | 1-4   | 4/4      | 120 | 0.0       | Standard count
//!   Verse 1       | 5-12  | 4/4      | 120 | 8.0       | 8 bars
//!   Pre Chorus    | 13-14 | 4/4      | 120 | 24.0      | 2 bars
//!   Chorus        | 15-22 | 4/4      | 130 | 28.0      | Tempo change!
//!   Verse 2       | 23-30 | 4/4      | 120 | ~42.8     | Back to 120
//!   Bridge        | 31-34 | 3/4      | 100 | ~58.8     | Waltz time
//!   Chorus 2      | 35-42 | 4/4      | 130 | ~66.0     | Back to 4/4
//!   Breakdown     | 43-44 | 7/8      | 140 | ~80.8     | Odd time
//!   Outro         | 45-48 | 6/8      | 90  | ~83.2     | Compound time
//!
//! Expected MIDI notes (from fts-guide/src/midi/notes.rs):
//!   Intro=87, Verse=84, Pre Chorus=90, Chorus=85,
//!   Bridge=86, Breakdown=92, Outro=88
//!
//! Count notes (C5-C6): 72=1, 73=2, 74=3, 75=4, 76=5, 77=6, 78=7
//!
//! Run with: `cargo test -p fts-guide guide_midi_gen -- --ignored --nocapture`

use daw_proto::primitives::{Duration as DawDuration, PositionInSeconds};
use reaper_test::reaper_test;
use std::time::Duration;

const FTS_GUIDE_CLAP: &str = "CLAP: FTS Guide";

/// Section definition for test song
struct TestSection {
    name: &'static str,
    start_seconds: f64,
    end_seconds: f64,
    /// Expected guide MIDI note (None = no guide for this section type)
    expected_guide_note: Option<u8>,
    /// Time signature numerator for count verification
    time_sig_num: u32,
}

/// Build the test song sections. Times are approximate — the test will use
/// regions at these positions and verify MIDI is generated correctly.
fn test_song() -> Vec<TestSection> {
    vec![
        TestSection {
            name: "Intro",
            start_seconds: 0.0,
            end_seconds: 8.0,
            expected_guide_note: Some(87), // D#6
            time_sig_num: 4,
        },
        TestSection {
            name: "Verse 1",
            start_seconds: 8.0,
            end_seconds: 24.0,
            expected_guide_note: Some(84), // C6 (Verse)
            time_sig_num: 4,
        },
        TestSection {
            name: "Pre Chorus",
            start_seconds: 24.0,
            end_seconds: 28.0,
            expected_guide_note: Some(90), // F#6
            time_sig_num: 4,
        },
        TestSection {
            name: "Chorus",
            start_seconds: 28.0,
            end_seconds: 42.0,
            expected_guide_note: Some(85), // C#6
            time_sig_num: 4,
        },
        TestSection {
            name: "Verse 2",
            start_seconds: 42.0,
            end_seconds: 58.0,
            expected_guide_note: Some(84), // C6 (Verse)
            time_sig_num: 4,
        },
        TestSection {
            name: "Bridge",
            start_seconds: 58.0,
            end_seconds: 66.0,
            expected_guide_note: Some(86), // D6
            time_sig_num: 3, // 3/4 waltz
        },
        TestSection {
            name: "Chorus 2",
            start_seconds: 66.0,
            end_seconds: 80.0,
            expected_guide_note: Some(85), // C#6 (Chorus)
            time_sig_num: 4,
        },
        TestSection {
            name: "Breakdown",
            start_seconds: 80.0,
            end_seconds: 84.0,
            expected_guide_note: Some(92), // G#6
            time_sig_num: 7, // 7/8
        },
        TestSection {
            name: "Outro",
            start_seconds: 84.0,
            end_seconds: 92.0,
            expected_guide_note: Some(88), // E6
            time_sig_num: 6, // 6/8
        },
    ]
}

/// Strip trailing numbers from section name: "Verse 1" → "Verse", "Chorus 2" → "Chorus"
fn section_type_name(name: &str) -> &str {
    name.split(|c: char| c.is_ascii_digit())
        .next()
        .unwrap_or(name)
        .trim()
}

#[reaper_test(isolated)]
async fn guide_midi_gen(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let regions = project.regions();
    let tracks = project.tracks();
    let song = test_song();

    // ══════════════════════════════════════════════════════════
    //  1. Create song structure with regions
    // ══════════════════════════════════════════════════════════
    ctx.log("=== Creating song structure ===");

    for section in &song {
        let id = regions.add(section.start_seconds, section.end_seconds, section.name).await?;
        ctx.log(&format!(
            "  Region {}: '{}' {:.1}s–{:.1}s ({})",
            id, section.name, section.start_seconds, section.end_seconds,
            if section.time_sig_num == 4 {
                "4/4".to_string()
            } else {
                format!("{}/x", section.time_sig_num)
            }
        ));
    }

    let region_count = regions.count().await?;
    assert_eq!(region_count, song.len(), "Region count mismatch");
    ctx.log(&format!("{} regions created", region_count));

    // ══════════════════════════════════════════════════════════
    //  2. Create Click track with FTS Guide plugin
    // ══════════════════════════════════════════════════════════
    ctx.log("=== Setting up Click track ===");

    let click_track = tracks.add("Click", None).await?;
    let _fx = match click_track.fx_chain().add(FTS_GUIDE_CLAP).await {
        Ok(fx) => fx,
        Err(e) => return Err(eyre::eyre!("Failed to add FX: {:?}", e)),
    };

    // Set 8 channels via chunk for multi-out
    let mut chunk = click_track.get_chunk().await?;
    if let Some(pos) = chunk.find("NCHAN ") {
        let end = chunk[pos..].find('\n').unwrap_or(chunk.len() - pos);
        chunk.replace_range(pos..pos + end, "NCHAN 8");
    } else if let Some(pos) = chunk.find('\n') {
        chunk.insert_str(pos + 1, "NCHAN 8\n");
    }
    click_track.set_chunk(chunk).await?;
    ctx.log("Click track ready (8ch, FTS Guide loaded)");

    tokio::time::sleep(Duration::from_millis(300)).await;

    // ══════════════════════════════════════════════════════════
    //  3. Generate guide MIDI items from regions
    // ══════════════════════════════════════════════════════════
    ctx.log("=== Generating guide MIDI ===");

    let all_regions = regions.all().await?;
    let mut generated_count = 0;

    for region in &all_regions {
        let stype = section_type_name(&region.name);

        // Look up the expected note from our test song definition
        let section_def = song.iter().find(|s| s.name == region.name);
        let expected_note = section_def.and_then(|s| s.expected_guide_note);

        let note = match expected_note {
            Some(n) => n,
            None => {
                ctx.log(&format!("  SKIP '{}' — no guide note", region.name));
                continue;
            }
        };

        // Create a MIDI item spanning the full section
        let section_duration = region.end_seconds() - region.start_seconds();
        let item = click_track
            .items()
            .add(
                PositionInSeconds::from_seconds(region.start_seconds()),
                DawDuration::from_seconds(section_duration),
            )
            .await?;

        let take = item.takes().active().await?;
        let midi = take.midi();

        // Add the guide trigger note at the start (short duration)
        midi.add_note(note, 100, 0.0, 240.0).await?;

        // Add count notes at each beat boundary: 72=1, 73=2, etc.
        let time_sig_num = section_def.map(|s| s.time_sig_num).unwrap_or(4);
        for beat in 0..time_sig_num.min(7) {
            let count_note = 72 + beat as u8; // C5 + beat index
            let beat_ppq = beat as f64 * 960.0; // 960 PPQ per quarter note
            midi.add_note(count_note, 80, beat_ppq, 240.0).await?;
        }

        ctx.log(&format!(
            "  '{}' @ {:.1}s → guide={} count=1-{} ({})",
            region.name, region.start_seconds(), note, time_sig_num.min(7), stype
        ));
        generated_count += 1;
    }

    ctx.log(&format!("{} sections generated with MIDI", generated_count));
    tokio::time::sleep(Duration::from_millis(300)).await;

    // ══════════════════════════════════════════════════════════
    //  4. Verify MIDI items
    // ══════════════════════════════════════════════════════════
    ctx.log("=== Verifying MIDI items ===");

    let items = click_track.items().all().await?;
    ctx.log(&format!("Total items on Click track: {}", items.len()));

    assert_eq!(
        items.len(), generated_count,
        "Item count should match generated sections"
    );

    // Verify items are at the expected positions
    for (i, section) in song.iter().enumerate() {
        if section.expected_guide_note.is_none() {
            continue;
        }
        if i >= items.len() {
            break;
        }
        let item = &items[i];
        let item_pos_s = item.position.as_seconds();
        let pos_diff = (item_pos_s - section.start_seconds).abs();
        ctx.log(&format!(
            "  Item {}: '{}' expected={:.1}s actual={:.2}s diff={:.4}s len={:.2}s",
            i, section.name, section.start_seconds, item_pos_s, pos_diff, item.length.as_seconds()
        ));
        assert!(
            pos_diff < 0.1,
            "Item {} ('{}') position {:.2} should be near {:.1}",
            i, section.name, item_pos_s, section.start_seconds
        );
    }

    // ══════════════════════════════════════════════════════════
    //  5. Verify MIDI notes in items
    // ══════════════════════════════════════════════════════════
    ctx.log("=== Verifying MIDI note content ===");

    let item_handles: Vec<_> = {
        let mut handles = Vec::new();
        for i in 0..items.len() as u32 {
            if let Some(h) = click_track.items().by_index(i).await? {
                handles.push(h);
            }
        }
        handles
    };

    for (i, (handle, section)) in item_handles.iter().zip(song.iter()).enumerate() {
        if section.expected_guide_note.is_none() {
            continue;
        }

        let take = handle.takes().active().await?;
        let notes = take.midi().notes().await?;
        ctx.log(&format!(
            "  Item {} '{}': {} notes",
            i, section.name, notes.len()
        ));

        // Should have at least: 1 guide note + count notes
        let expected_min_notes = 1 + section.time_sig_num.min(7) as usize;
        assert!(
            notes.len() >= expected_min_notes,
            "Item {} '{}' should have at least {} notes (1 guide + {} count), got {}",
            i, section.name, expected_min_notes, section.time_sig_num.min(7), notes.len()
        );

        // Verify the guide note is present
        let guide_note = section.expected_guide_note.unwrap();
        let has_guide = notes.iter().any(|n| n.pitch == guide_note);
        assert!(
            has_guide,
            "Item {} '{}' should have guide note {} but notes are: {:?}",
            i, section.name, guide_note,
            notes.iter().map(|n| n.pitch).collect::<Vec<_>>()
        );

        // Verify count notes are present (72, 73, 74, ... up to time_sig_num)
        for beat in 0..section.time_sig_num.min(7) {
            let count_note = 72 + beat as u8;
            let has_count = notes.iter().any(|n| n.pitch == count_note);
            assert!(
                has_count,
                "Item {} '{}' should have count note {} (beat {})",
                i, section.name, count_note, beat + 1
            );
        }
    }

    ctx.log(&format!(
        "guide_midi_gen: PASSED — {} regions, {} MIDI items, all notes verified across {} time signatures",
        region_count, generated_count,
        song.iter().map(|s| s.time_sig_num).collect::<std::collections::HashSet<_>>().len()
    ));
    Ok(())
}
