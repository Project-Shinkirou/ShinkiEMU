use super::*;

impl CmvsScene {
    /// A CMV provider owns its latest decoded pixels independently of the
    /// archive-image cache. Dropping the controller freezes this provider.
    pub(crate) fn bind_cmv_frame(&mut self, parent: u8, uri: &str, frame: TextureFrame) {
        self.bind_memory_image(CmvsTextureSlot::Parent(parent), uri, frame);
    }

    pub(crate) fn bind_memory_image(
        &mut self,
        slot: CmvsTextureSlot,
        uri: &str,
        frame: TextureFrame,
    ) {
        if self.bindings.get(&slot).is_none_or(|bound| bound != uri) {
            self.bindings.insert(slot, uri.to_owned());
        }
        self.dynamic_images.insert(slot, frame);
        if slot.is_root() {
            self.filtered_images.remove(&slot);
        }
    }
}
