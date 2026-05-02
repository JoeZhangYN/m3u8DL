use m3u8dl_server::adapters::m3u8_normalize::normalize;

#[test]
fn appends_endlist_when_missing() {
    let input = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.76,\nseg0.ts\n";
    let out = normalize(input);
    assert!(out.contains("#EXT-X-ENDLIST"), "got: {out}");
    assert!(out.trim_end().ends_with("#EXT-X-ENDLIST"), "got: {out}");
}

#[test]
fn leaves_endlist_alone_when_present() {
    let input = "#EXTM3U\n#EXTINF:5.76,\nseg0.ts\n#EXT-X-ENDLIST\n";
    let out = normalize(input);
    let count = out.matches("#EXT-X-ENDLIST").count();
    assert_eq!(count, 1, "duplicated ENDLIST in: {out}");
}

#[test]
fn splits_one_line_blob_at_ext_directives() {
    // DevTools "copy as text" typically preserves spaces between fields, just drops newlines
    let oneliner = "#EXTM3U #EXT-X-VERSION:3 #EXT-X-TARGETDURATION:6 #EXTINF:5.76,seg0.ts #EXT-X-ENDLIST";
    let out = normalize(oneliner);
    assert!(out.contains("\n#EXT-X-VERSION"), "got: {out}");
    assert!(out.contains("\n#EXTINF"), "got: {out}");
    assert!(out.contains("\n#EXT-X-ENDLIST"), "got: {out}");
}

#[test]
fn splits_one_line_blob_before_https_urls() {
    let oneliner = "#EXTM3U #EXTINF:5.76, https://cdn.example.com/seg0.ts #EXT-X-ENDLIST";
    let out = normalize(oneliner);
    assert!(out.contains("\nhttps://cdn.example.com/seg0.ts"), "got: {out}");
}

#[test]
fn does_not_split_when_already_multiline() {
    let already_ok = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:5.76,\nseg0.ts\n#EXT-X-ENDLIST\n";
    let out = normalize(already_ok);
    // line count should be similar, no spurious splits
    let original_lines = already_ok.lines().count();
    let out_lines = out.lines().count();
    assert!(out_lines >= original_lines, "lost lines: {original_lines} -> {out_lines}");
}

#[test]
fn idempotent_apply_twice() {
    let input = "#EXTM3U #EXTINF:5.76,seg0.ts";
    let once = normalize(input);
    let twice = normalize(&once);
    assert_eq!(
        once.trim_end(),
        twice.trim_end(),
        "not idempotent:\nonce={once:?}\ntwice={twice:?}"
    );
}
