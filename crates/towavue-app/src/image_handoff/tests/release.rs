use super::*;

#[test]
fn final_image_close_releases_originals_with_other_media_tabs_remaining() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::release::final_image_close_releases_originals_with_other_media_tabs_remaining",
    ) else {
        return;
    };
    for kind in [MediaKind::Audio, MediaKind::Video] {
        for active_image in [false, true] {
            let (mut app, context, image_tab) = fixture(&root);
            let cached_path = root.join("cached.bmp");
            crate::tab_transfer::tests::bitmap(&cached_path);
            app.image_loader.request(vec![cached_path]);
            let deadline = Instant::now() + Duration::from_secs(5);
            let loaded = loop {
                if let Some(loaded) = app.image_loader.take_completed() {
                    break loaded;
                }
                assert!(Instant::now() < deadline, "original cache seed timeout");
                std::thread::sleep(Duration::from_millis(1));
            };
            let cached = Arc::downgrade(loaded.images[0].1.as_ref().expect("decoded cache seed"));
            drop(loaded);
            let image = app.image.as_ref().expect("original").clone();
            let weak = Arc::downgrade(&image.decoded);
            let texture = image.texture.id();
            app.image_texture_cache.entries.push_back(image.into());
            frame(&mut app, &context);
            let path = root.join(if kind == MediaKind::Audio {
                "other.wav"
            } else {
                "other.mp4"
            });
            let other_tab = app.tabs.open_new(path.clone(), kind);
            app.load_path(path.clone(), kind);
            let instance = app.media_generation;
            let state = app.state;
            let generation = app.generation;
            let error = app.playback_error.clone();
            assert!(weak.upgrade().is_some());
            assert!(
                cached.upgrade().is_some(),
                "ordinary departure retains the decoded cache"
            );
            if active_image {
                app.activate_tab(image_tab);
            }
            app.close_tab_unchecked(image_tab);
            assert_eq!(app.tabs.active().expect("remaining media").id, other_tab);
            assert_eq!(app.path.as_ref(), Some(&path));
            assert_eq!(app.media_generation, instance);
            assert_eq!(app.generation, generation);
            assert_eq!(app.state, state);
            assert_eq!(app.playback_error, error);
            assert!(app.image_texture_cache.entries.is_empty());
            assert!(weak.upgrade().is_none());
            let deadline = Instant::now() + Duration::from_secs(5);
            while cached.upgrade().is_some() {
                assert!(
                    Instant::now() < deadline,
                    "unused decoded cache was not cleared"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(app.retained_images.is_empty());
            assert!(
                frame(&mut app, &context)
                    .textures_delta
                    .free
                    .contains(&texture)
            );
            assert!(context.tex_manager().read().meta(texture).is_none());
            let image_generation = app.image_generation;
            let extra = app.tabs.open_new(root.join("extra-media"), kind);
            app.tabs.activate(other_tab);
            app.close_tab_unchecked(extra);
            assert_eq!(
                app.image_generation, image_generation,
                "unrelated media close must not reset image work"
            );
        }
    }
}

#[test]
fn last_tab_close_releases_cached_originals_but_other_tabs_keep_them() {
    let Some(root) = crate::tests::isolated_test_root(
        "image_handoff::tests::release::last_tab_close_releases_cached_originals_but_other_tabs_keep_them",
    ) else {
        return;
    };
    let (mut app, context, tab) = fixture(&root);
    let original = app.image.as_ref().expect("original").clone();
    let retained = Arc::downgrade(&original.decoded);
    let texture = original.texture.id();
    app.image_texture_cache.entries.push_back(original.into());
    let unused = decoded(32, 32, [80, 90, 100, 255]);
    let unused_weak = Arc::downgrade(&unused);
    let unused_texture = app
        .image_texture_cache
        .load(&context, &root.join("unused.png"), unused)
        .expect("unused cached original")
        .texture
        .id();
    frame(&mut app, &context);

    let other = app.tabs.open_new(root.join("other.png"), MediaKind::Image);
    app.tabs.activate(tab);
    app.close_tab_unchecked(other);
    assert_eq!(
        app.image.as_ref().expect("live original").texture.id(),
        texture
    );
    assert!(retained.upgrade().is_some() && unused_weak.upgrade().is_none());
    assert_eq!(app.image_texture_cache.entries.len(), 2);

    let other_path = root.join("new-active.png");
    let other = app.tabs.open_new(other_path.clone(), MediaKind::Image);
    app.load_path(other_path, MediaKind::Image);
    app.image_loader.request(Vec::new());
    assert_eq!(
        app.retained_images[&tab]
            .image
            .as_ref()
            .expect("inactive original")
            .texture
            .id(),
        texture
    );
    app.close_tab_unchecked(other);
    assert_eq!(
        app.image.as_ref().expect("restored original").texture.id(),
        texture
    );
    assert_eq!(app.image_texture_cache.entries.len(), 2);

    app.close_tab_unchecked(tab);
    assert!(app.image_texture_cache.entries.is_empty());
    assert!(retained.upgrade().is_none() && unused_weak.upgrade().is_none());
    let output = frame(&mut app, &context);
    for id in [texture, unused_texture] {
        assert!(context.tex_manager().read().meta(id).is_none());
        assert!(output.textures_delta.free.contains(&id));
    }
    assert!(app.image_loader.take_completed().is_none());
}
