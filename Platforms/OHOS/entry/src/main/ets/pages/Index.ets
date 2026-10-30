import { common } from '@kit.AbilityKit';
import { BusinessError } from '@kit.BasicServicesKit';
import manager from 'libshinki_emu_manager.so';
import { extractBundled, listen, reportConfiguration } from '../bridge/ManagerBridge';

// OH_NativeXComponent_KeyCode KEY_BACK; the Manager maps it to Escape.
const KEY_BACK = 2;

@Entry
@Component
struct Index {
  @State private failure: string = '';
  @State private ready: boolean = false;
  private context = getContext(this) as common.UIAbilityContext;
  private started: boolean = false;

  aboutToAppear(): void {
    try {
      extractBundled(this.context);
      listen(this.context);
      reportConfiguration(this.context.config.colorMode);
      this.ready = true;
    } catch (error) {
      this.failure = `SHINKI_EMU_OHOS_BUNDLED: ${(error as BusinessError).message}`;
    }
  }

  onBackPress(): boolean {
    manager.pressKey(KEY_BACK, true);
    manager.pressKey(KEY_BACK, false);
    return true;
  }

  build() {
    Stack() {
      if (this.ready) {
        XComponent({ id: 'shinki-emu-manager', type: XComponentType.SURFACE, libraryname: 'shinki_emu_manager' })
          .width('100%')
          .height('100%')
          .focusable(true)
          .defaultFocus(true)
          .onLoad(() => {
            if (!this.started) {
              this.started = true;
              manager.start(this.context.filesDir);
            }
          })
      } else {
        Text(this.failure)
          .fontSize(16)
          .fontColor(Color.White)
          .padding(24)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#10131A')
  }
}
