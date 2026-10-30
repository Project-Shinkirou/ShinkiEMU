import { fileIo } from '@kit.CoreFileKit';
import { common } from '@kit.AbilityKit';
import { promptAction } from '@kit.ArkUI';
import { BusinessError } from '@kit.BasicServicesKit';
import manager from 'libshinki_emu_manager.so';

@Entry
@Component
struct Index {
  @State private status: string = 'Starting ShinkiEMU Manager…';
  private context = getContext(this) as common.UIAbilityContext;
  private started: boolean = false;

  aboutToAppear(): void {
    manager.setEventListener((kind: string, payload: string) => {
      if (kind === 'error') {
        this.status = payload;
        promptAction.showToast({ message: payload, duration: 5000 });
      } else if (kind === 'pickFolder' || kind === 'pickFile' || kind === 'pickDirectory') {
        const request = JSON.parse(payload) as { id: number, title: string, extensions: string[] };
        manager.picker(kind, JSON.stringify({ title: request.title, extensions: request.extensions, requestId: request.id }));
      } else if (kind === 'restart') {
        this.status = 'Restart the application to apply changes.';
      } else if (kind === 'openPath') {
        this.status = payload;
      }
    });
  }

  private async pick(kind: string, payload: string): Promise<void> {
    const request = JSON.parse(payload) as { id: number, title: string, extensions: string[] };
    try {
      const picker = await import('@kit.CoreFileKit');
      const result = kind === 'pickFolder' || kind === 'pickDirectory'
        ? await new picker.FolderPicker().select(this.context)
        : await new picker.DocumentViewPicker().select(this.context, { maxSelectNumber: 1 });
      manager.completePicker(request.id, result.length > 0 ? result[0] : '', result.length > 0 ? '' : 'cancelled');
    } catch (error) {
      manager.completePicker(request.id, '', (error as BusinessError).message ?? 'picker failed');
    }
  }

  @Builder
  private nativeSurface(): void {
    XComponent({ id: 'shinki-manager-surface', type: XComponentType.SURFACE, libraryName: 'shinki_emu_manager' })
      .width('100%')
      .height('100%')
      .onLoad(() => {
        if (!this.started) {
          this.started = true;
          manager.start(this.context.filesDir);
        }
      })
      .onDestroy(() => manager.lifecycle(false, display.getDefaultDisplaySync().densityDPI / 160.0));
  }

  build(): void {
    Column() {
      this.nativeSurface();
      if (!this.started) {
        Text(this.status).fontSize(18).fontColor(Color.White).padding(24);
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#10131A');
  }

  onPageShow(): void {
    manager.lifecycle(true, display.getDefaultDisplaySync().densityDPI / 160.0);
  }

  onPageHide(): void {
    manager.lifecycle(false, display.getDefaultDisplaySync().densityDPI / 160.0);
  }
}
