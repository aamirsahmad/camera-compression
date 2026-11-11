//! End-to-end tests over real JPEGs, synthesized per test.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use camera_compression::{
    Chroma, Config, MetadataPolicy, Outcome, Progress, Silent, batch, orient, pipeline, source,
};
use common::{at, dimensions, listing, orientation_of, write_jpeg, write_jpeg_with_orientation};

/// Counts callbacks so tests can assert the library reports progress.
#[derive(Default)]
struct Counting {
    advanced: AtomicUsize,
    failures: AtomicUsize,
}

impl Progress for Counting {
    fn advance(&self) {
        self.advanced.fetch_add(1, Ordering::Relaxed);
    }

    fn file_failed(&self, _message: &str) {
        self.failures.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn compresses_a_single_image_and_writes_a_decodable_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/shot.jpg");
    write_jpeg(&input, 1200, 800);

    let config = Config::new(at(dir.path(), "out"));
    let outcome = pipeline::process(&input, &config).unwrap();

    assert!(matches!(outcome, Outcome::Written { bytes } if bytes > 0));
    let written = std::fs::read(at(dir.path(), "out/shot.jpg")).unwrap();
    assert_eq!(dimensions(&written), (1200, 800), "should not upscale");
}

#[test]
fn downscales_to_the_width_bound_preserving_aspect() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/wide.jpg");
    write_jpeg(&input, 3000, 2000);

    let config = Config {
        max_width: Some(1500),
        ..Config::new(at(dir.path(), "out"))
    };
    pipeline::process(&input, &config).unwrap();

    let written = std::fs::read(at(dir.path(), "out/wide.jpg")).unwrap();
    assert_eq!(dimensions(&written), (1500, 1000));
}

#[test]
fn respects_both_bounds_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/tall.jpg");
    write_jpeg(&input, 1000, 2000);

    let config = Config {
        max_width: Some(600),
        max_height: Some(600),
        ..Config::new(at(dir.path(), "out"))
    };
    pipeline::process(&input, &config).unwrap();

    let (w, h) = dimensions(&std::fs::read(at(dir.path(), "out/tall.jpg")).unwrap());
    assert!(w <= 600 && h <= 600, "got {w}x{h}");
    assert_eq!((w, h), (300, 600), "height should be the binding bound");
}

#[test]
fn rotates_portrait_frames_upright_and_clears_the_orientation_tag() {
    let dir = tempfile::tempdir().unwrap();
    // Stored landscape, tagged as needing a 90-degree rotation.
    let input = at(dir.path(), "in/portrait.jpg");
    write_jpeg_with_orientation(&input, 3000, 2000, 8);

    assert_eq!(orient::read(&std::fs::read(&input).unwrap()), 8);

    let config = Config {
        max_width: Some(1000),
        ..Config::new(at(dir.path(), "out"))
    };
    pipeline::process(&input, &config).unwrap();

    let out = at(dir.path(), "out/portrait.jpg");
    let (w, h) = dimensions(&std::fs::read(&out).unwrap());
    assert_eq!(
        (w, h),
        (1000, 1500),
        "bounds apply to the upright image, so output must be portrait"
    );
    assert_eq!(
        orientation_of(&out),
        Some(orient::UPRIGHT),
        "pixels are already rotated, so the tag must not rotate again"
    );
}

#[test]
fn leaves_already_upright_images_untransposed() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/landscape.jpg");
    write_jpeg_with_orientation(&input, 3000, 2000, 1);

    let config = Config {
        max_width: Some(1000),
        ..Config::new(at(dir.path(), "out"))
    };
    pipeline::process(&input, &config).unwrap();

    let written = std::fs::read(at(dir.path(), "out/landscape.jpg")).unwrap();
    assert_eq!(dimensions(&written), (1000, 667));
}

#[test]
fn preserves_exif_by_default_and_drops_it_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/tagged.jpg");
    write_jpeg_with_orientation(&input, 800, 600, 1);

    let kept = Config::new(at(dir.path(), "kept"));
    pipeline::process(&input, &kept).unwrap();
    assert!(
        orientation_of(&at(dir.path(), "kept/tagged.jpg")).is_some(),
        "EXIF should survive by default"
    );

    let stripped = Config {
        metadata: MetadataPolicy::none(),
        ..Config::new(at(dir.path(), "stripped"))
    };
    pipeline::process(&input, &stripped).unwrap();
    assert!(
        orientation_of(&at(dir.path(), "stripped/tagged.jpg")).is_none(),
        "--strip-metadata should remove EXIF entirely"
    );
}

#[test]
fn stripping_metadata_produces_a_smaller_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/tagged.jpg");
    write_jpeg_with_orientation(&input, 800, 600, 1);

    for (name, policy) in [
        ("kept", MetadataPolicy::default()),
        ("bare", MetadataPolicy::none()),
    ] {
        let config = Config {
            metadata: policy,
            ..Config::new(at(dir.path(), name))
        };
        pipeline::process(&input, &config).unwrap();
    }

    let kept = std::fs::metadata(at(dir.path(), "kept/tagged.jpg"))
        .unwrap()
        .len();
    let bare = std::fs::metadata(at(dir.path(), "bare/tagged.jpg"))
        .unwrap()
        .len();
    assert!(
        bare < kept,
        "stripping metadata should save bytes: {bare} vs {kept}"
    );
}

#[test]
fn skips_existing_output_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/shot.jpg");
    write_jpeg(&input, 600, 400);
    let config = Config::new(at(dir.path(), "out"));

    assert!(matches!(
        pipeline::process(&input, &config).unwrap(),
        Outcome::Written { .. }
    ));
    assert_eq!(
        pipeline::process(&input, &config).unwrap(),
        Outcome::SkippedExists,
        "a second run should resume, not redo work"
    );

    let forced = Config {
        force: true,
        ..config
    };
    assert!(matches!(
        pipeline::process(&input, &forced).unwrap(),
        Outcome::Written { .. }
    ));
}

#[test]
fn lower_quality_and_more_subsampling_both_shrink_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/shot.jpg");
    write_jpeg(&input, 1200, 800);

    let size_of = |name: &str, config: &Config| {
        pipeline::process(&input, config).unwrap();
        std::fs::metadata(at(dir.path(), &format!("{name}/shot.jpg")))
            .unwrap()
            .len()
    };

    let high = size_of(
        "high",
        &Config {
            quality: 95,
            chroma: Chroma::None,
            ..Config::new(at(dir.path(), "high"))
        },
    );
    let low = size_of(
        "low",
        &Config {
            quality: 60,
            chroma: Chroma::Quarter,
            ..Config::new(at(dir.path(), "low"))
        },
    );

    assert!(
        low < high,
        "q60/4:2:0 should be smaller than q95/4:4:4: {low} vs {high}"
    );
}

#[test]
fn a_corrupt_input_fails_without_leaving_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/truncated.jpg");
    write_jpeg(&input, 800, 600);
    // Cut inside the header tables, so the file is undecodable rather than
    // merely incomplete. zune-jpeg tolerates truncated *scan* data and returns a
    // partial image, so lopping off the tail would not reliably fail here.
    let full = std::fs::read(&input).unwrap();
    std::fs::write(&input, &full[..64]).unwrap();

    let out_dir = at(dir.path(), "out");
    let config = Config::new(&out_dir);
    assert!(pipeline::process(&input, &config).is_err());

    std::fs::create_dir_all(&out_dir).unwrap();
    assert!(
        listing(&out_dir).is_empty(),
        "a failed encode must not leave .part or .jpg files behind"
    );
}

#[test]
fn batch_run_reports_written_skipped_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = (0..5)
        .map(|i| {
            let p = at(dir.path(), &format!("in/shot{i}.jpg"));
            write_jpeg(&p, 400, 300);
            p
        })
        .collect();

    let config = Config::new(at(dir.path(), "out"));
    let progress = Counting::default();
    let stats = batch::run(&files, &config, &progress).unwrap();

    assert_eq!(stats.written, 5);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.attempted(), 5);
    assert!(stats.bytes > 0);
    assert!(!stats.had_failures());
    assert_eq!(progress.advanced.load(Ordering::Relaxed), 5);
    assert_eq!(listing(&at(dir.path(), "out")).len(), 5);

    // Re-running should skip every file.
    let again = batch::run(&files, &config, &Silent).unwrap();
    assert_eq!(again.skipped, 5);
    assert_eq!(again.written, 0);
}

#[test]
fn batch_run_creates_a_nested_output_directory() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/shot.jpg");
    write_jpeg(&input, 400, 300);

    let out = at(dir.path(), "a/b/c");
    let stats = batch::run(&[input], &Config::new(&out), &Silent).unwrap();

    assert_eq!(stats.written, 1);
    assert!(out.is_dir());
}

#[test]
fn keep_going_processes_the_rest_after_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let good_a = at(dir.path(), "in/a.jpg");
    let good_b = at(dir.path(), "in/b.jpg");
    let bad = at(dir.path(), "in/bad.jpg");
    write_jpeg(&good_a, 400, 300);
    write_jpeg(&good_b, 400, 300);
    std::fs::write(&bad, b"not a jpeg").unwrap();

    let files = vec![bad, good_a, good_b];
    let config = Config {
        keep_going: true,
        ..Config::new(at(dir.path(), "out"))
    };

    let progress = Counting::default();
    let stats = batch::run(&files, &config, &progress).unwrap();

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.written, 2, "the good files should still be processed");
    assert!(stats.had_failures());
    assert_eq!(progress.failures.load(Ordering::Relaxed), 1);
}

#[test]
fn a_failure_is_counted_rather_than_returned_as_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = at(dir.path(), "in/bad.jpg");
    std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
    std::fs::write(&bad, b"not a jpeg").unwrap();

    let config = Config::new(at(dir.path(), "out"));
    // The run itself succeeds; the caller decides that `failed > 0` is fatal.
    let stats = batch::run(&[bad], &config, &Silent).unwrap();
    assert_eq!(stats.failed, 1);
    assert!(stats.had_failures());
}

#[test]
fn walking_a_nested_tree_finds_every_jpg_and_flattens_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = at(dir.path(), "camera");
    write_jpeg(&at(&root, "24/pictures/a.JPG"), 400, 300);
    write_jpeg(&at(&root, "25/pictures/b.jpg"), 400, 300);
    write_jpeg(&at(&root, "25/deeper/still/c.Jpg"), 400, 300);
    std::fs::create_dir_all(at(&root, "25/pictures")).unwrap();
    std::fs::write(at(&root, "25/pictures/raw.ARW"), b"raw").unwrap();

    let files = source::collect(&root).unwrap();
    assert_eq!(files.len(), 3, "found {files:?}");

    let out = at(dir.path(), "out");
    let stats = batch::run(&files, &Config::new(&out), &Silent).unwrap();
    assert_eq!(stats.written, 3);
    assert_eq!(listing(&out), vec!["a.jpg", "b.jpg", "c.jpg"]);
}

#[test]
fn collisions_are_detected_before_any_work_happens() {
    let dir = tempfile::tempdir().unwrap();
    let root = at(dir.path(), "camera");
    write_jpeg(&at(&root, "24/SHOT.JPG"), 400, 300);
    write_jpeg(&at(&root, "25/SHOT.jpg"), 400, 300);
    write_jpeg(&at(&root, "25/OTHER.jpg"), 400, 300);

    let files = source::collect(&root).unwrap();
    assert_eq!(source::colliding_stems(&files), vec!["SHOT"]);
}

#[test]
fn a_file_list_source_is_accepted_alongside_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let input = at(dir.path(), "in/shot.jpg");
    write_jpeg(&input, 400, 300);

    let list = at(dir.path(), "list.txt");
    std::fs::write(&list, format!("# a comment\n\n{}\n", input.display())).unwrap();

    let files = source::collect(&list).unwrap();
    assert_eq!(files, vec![input]);
}
