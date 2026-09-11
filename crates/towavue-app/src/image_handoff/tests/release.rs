use super::*;

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
    app.image_texture_cache.entries.push_back(original);
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
    assert!(retained.upgrade().is_some() && unused_weak.upgrade().is_some());
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
