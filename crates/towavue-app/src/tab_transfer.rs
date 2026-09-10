use super::*;

#[cfg(test)]
#[path = "tab_transfer_tests.rs"]
pub(crate) mod tests;

#[derive(Clone)]
pub(super) struct DetachRequest {
    pub tab: TabId,
    path: PathBuf,
    instance: u64,
}

pub(super) struct TabTransfer {
    target: towavue_core::TabTarget,
    media: MediaTransfer,
    edits: Option<EditHistory>,
    export_path: Option<PathBuf>,
    audio_options: Option<AudioExportOptions>,
    metadata_options: Option<MetadataExportOptions>,
    audio_queue: Option<audio_playback::AudioTab>,
    focus: Option<egui::Id>,
    timeline: Option<egui::containers::panel::PanelState>,
}

enum MediaTransfer {
    Playback(Box<playback_tab::RetainedPlaybackTab>),
    Image(Box<RetainedImageTab>),
}

pub(super) struct ImageStage {
    image: Option<ImagePresentation>,
    pages: Vec<Result<ImagePresentation, String>>,
    previews: BTreeMap<PathBuf, ImagePreviewPresentation>,
}

impl ImagePresentation {
    fn rebind(&self, context: &egui::Context, path: &Path) -> Result<Self, String> {
        let limit = context.input(|input| input.max_texture_side);
        if self
            .decoded
            .frames
            .iter()
            .any(|frame| frame.width as usize > limit || frame.height as usize > limit)
        {
            return Err(format!(
                "Image dimensions exceed this graphics device's {limit}px texture limit"
            ));
        }
        let sampling = self.sampling.get();
        Ok(Self {
            decoded: Arc::clone(&self.decoded),
            texture: context.load_texture(
                format!("image:{}", path.display()),
                color_image(&self.decoded.frames[self.frame_index]),
                sampling,
            ),
            frame_index: self.frame_index,
            next_frame_at: self.next_frame_at,
            sampling: std::rc::Rc::new(std::cell::Cell::new(sampling)),
        })
    }
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn validate_transfer_window(&self) -> Result<(), String> {
        if self.exit_requested || self.modal_input_blocked() {
            return Err("close the dialog before moving a tab".into());
        }
        if self.renderer.is_none()
            || self.ui_context.is_none()
            || self.graphics_recovery_request.is_some()
        {
            return Err("wait for the window's graphics device to become available".into());
        }
        Ok(())
    }

    pub(super) fn tab_detach_request(&self, id: TabId) -> Result<DetachRequest, String> {
        self.validate_transfer_window()?;
        let tab = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .ok_or("the tab is no longer open")?;
        if self
            .active_export
            .as_ref()
            .is_some_and(|export| export.tab == id)
        {
            return Err("wait for this tab's export to finish".into());
        }
        let path = tab.target.current_path();
        let instance = if self.displayed_tab == Some(id) && self.path.as_deref() == Some(path) {
            self.media_generation
        } else if tab.target.media_kind() == MediaKind::Image {
            self.retained_images
                .get(&id)
                .filter(|saved| saved.path == path)
                .ok_or("the tab's image state is unavailable")?
                .instance
        } else {
            self.retained_playback
                .get(&id)
                .filter(|saved| saved.path == path)
                .ok_or("the tab's playback state is unavailable")?
                .instance
        };
        Ok(DetachRequest {
            tab: id,
            path: path.to_owned(),
            instance,
        })
    }

    pub(super) fn validate_tab_transfer(&self, request: &DetachRequest) -> Result<(), String> {
        let current = self.tab_detach_request(request.tab)?;
        if current.path != request.path || current.instance != request.instance {
            return Err("the tab changed before it could be moved".into());
        }
        Ok(())
    }

    pub(super) fn prepare_image_transfer(
        &self,
        id: TabId,
        context: &egui::Context,
    ) -> Result<Option<ImageStage>, String> {
        let tab = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .expect("validated tab");
        if tab.target.media_kind() != MediaKind::Image {
            return Ok(None);
        }
        let path = tab.target.current_path();
        let (image, pages, previews) = if self.displayed_tab == Some(id) {
            (&self.image, &self.reading_pages, &self.image_previews)
        } else {
            let saved = &self.retained_images[&id];
            (&saved.image, &saved.reading_pages, &saved.previews)
        };
        let image = image
            .as_ref()
            .map(|image| image.rebind(context, path))
            .transpose()?;
        let pages = pages
            .iter()
            .map(|page| match page {
                Ok(image) => image.rebind(context, path).map(Ok),
                Err(error) => Ok(Err(error.clone())),
            })
            .collect::<Result<Vec<_>, String>>()?;
        let limit = context.input(|input| input.max_texture_side);
        let previews = previews
            .iter()
            .map(|(path, preview)| {
                if preview.pixels.size.iter().any(|side| *side > limit) {
                    return Err("Loading preview exceeds the destination texture limit".to_owned());
                }
                let pixels = Arc::clone(&preview.pixels);
                Ok((
                    path.clone(),
                    ImagePreviewPresentation {
                        texture: context.load_texture(
                            format!("loading-preview:{}", path.display()),
                            Arc::clone(&pixels),
                            TextureOptions::LINEAR,
                        ),
                        pixels,
                        source_size: preview.source_size,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        Ok(Some(ImageStage {
            image,
            pages,
            previews,
        }))
    }

    pub(super) fn take_tab_transfer(
        &mut self,
        request: &DetachRequest,
        stage: Option<ImageStage>,
    ) -> TabTransfer {
        let id = request.tab;
        let target = self
            .tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == id)
            .expect("validated tab")
            .target
            .clone();
        let media = if target.media_kind() == MediaKind::Image {
            let mut saved = if self.displayed_tab == Some(id) {
                self.take_image_tab_state()
            } else {
                self.retained_images.remove(&id).expect("validated image")
            };
            let stage = stage.expect("prepared image textures");
            saved.image = stage.image;
            saved.reading_pages = stage.pages;
            saved.previews = stage.previews;
            MediaTransfer::Image(Box::new(saved))
        } else {
            let mut playback = if self.displayed_tab == Some(id) {
                self.cancel_hold_speed();
                self.cancel_frame_steps();
                self.take_playback_tab_state()
            } else {
                self.retained_playback
                    .remove(&id)
                    .expect("validated playback")
            };
            // egui texture and widget IDs belong to the source context, unlike the
            // runtime frame, which remains on the host's shared D3D11 device.
            playback.waveform = None;
            playback.playlist.detach_context();
            self.duration_workers.remove(&playback.instance);
            MediaTransfer::Playback(Box::new(playback))
        };
        let focus = self
            .ui_context
            .as_ref()
            .and_then(|context| tab_focus::take(context, id));
        let timeline = self.ui_context.as_ref().and_then(|context| {
            context.data_mut(|data| {
                data.get_persisted::<egui::containers::panel::PanelState>(egui::Id::new((
                    "timeline", id,
                )))
            })
        });
        let transfer = TabTransfer {
            target,
            media,
            edits: self.edits.remove(&id),
            export_path: self.export_paths.remove(&id),
            audio_options: self.audio_export_settings.remove(&id),
            metadata_options: self.metadata_export_settings.remove(&id),
            audio_queue: self.audio_queues.remove(&id),
            focus,
            timeline,
        };
        self.remove_tab(id, false);
        transfer
    }

    pub(super) fn accept_tab_transfer(&mut self, mut transfer: TabTransfer, gap: usize) -> TabId {
        let path = transfer.target.current_path().to_owned();
        let kind = transfer.target.media_kind();
        let id = self.tabs.open_new(path.clone(), kind);
        self.tabs.get_mut(id).expect("new tab").target = transfer.target;
        self.tabs.reorder(id, gap);
        // Reserve an identity without changing the still-displayed tab's instance;
        // load_path must retain that tab under its existing worker identity.
        self.media_sequence = self
            .media_sequence
            .max(self.media_generation)
            .wrapping_add(1);
        let old_instance = match &mut transfer.media {
            MediaTransfer::Playback(saved) => {
                let old = saved.instance;
                saved.instance = self.media_sequence;
                saved.graphics_epoch = self.graphics_epoch;
                old
            }
            MediaTransfer::Image(saved) => {
                let old = saved.instance;
                saved.instance = self.media_sequence;
                saved.graphics_epoch = self.graphics_epoch;
                old
            }
        };
        if let Some(edits) = transfer.edits {
            self.edits.insert(id, edits);
        }
        if let Some(path) = transfer.export_path {
            self.export_paths.insert(id, path);
        }
        if let Some(options) = transfer.audio_options {
            self.audio_export_settings.insert(id, options);
        }
        if let Some(options) = transfer.metadata_options {
            self.metadata_export_settings.insert(id, options);
        }
        if let Some(mut queue) = transfer.audio_queue {
            let notify = Arc::clone(&self.notify);
            queue.transfer(old_instance, self.media_sequence, move || {
                notify(AppEvent::FolderReady)
            });
            self.audio_queues.insert(id, queue);
        }
        match transfer.media {
            MediaTransfer::Playback(saved) => {
                self.retained_playback.insert(id, *saved);
            }
            MediaTransfer::Image(saved) => {
                self.retained_images.insert(id, *saved);
            }
        }
        self.load_path_with_transfer(path, kind, true);
        if let Some(context) = &self.ui_context {
            if let Some(focus) = transfer.focus {
                tab_focus::adopt(context, id, focus);
            }
            if let Some(timeline) = transfer.timeline {
                context.data_mut(|data| {
                    data.insert_persisted(egui::Id::new(("timeline", id)), timeline)
                });
            }
        }
        id
    }
}
