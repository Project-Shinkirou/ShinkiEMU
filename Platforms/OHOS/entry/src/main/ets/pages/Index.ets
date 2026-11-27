import { common } from '@kit.AbilityKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import manager from 'libshinki_emu_manager.so';
import { extractBundled, listen, reportConfiguration } from '../bridge/ManagerBridge';

// OH_NativeXComponent_KeyCode KEY_BACK; the Manager maps it to Escape.
const KEY_BACK = 2;
const STAGE_FOCUS = 'manager-stage-focus';
const EDITOR_FOCUS = 'session-text-focus';

@Entry
@Component
struct Index {
  @State private failure: string = '';
  @State private ready: boolean = false;
  @State private textTarget: string = '';
  @State private editing: boolean = false;
  @State private draft: string = '';
  private editTarget: string = '';
  private editorFocusPending: boolean = false;
  private context = getContext(this) as common.UIAbilityContext;

  aboutToAppear(): void {
    try {
      extractBundled(this.context);
      listen(this.context, (epoch: string) => {
        if (epoch !== this.textTarget) {
          if (this.editing) {
            this.finishEditor();
          }
          this.textTarget = epoch;
        }
      });
    } catch (error) {
      this.failure = `SHINKI_EMU_OHOS_BUNDLED: ${(error as BusinessError).message}`;
      return;
    }
    try {
      // Set up the native event queue before mounting the XComponent.
      manager.start(this.context.filesDir, this.context.cacheDir);
      reportConfiguration(this.context.config.colorMode);
      this.ready = true;
    } catch (error) {
      this.failure = `SHINKI_EMU_OHOS_STARTUP: ${(error as BusinessError).message}`;
    }
  }

  onPageHide(): void {
    this.closeEditor();
  }

  private closeEditor(): void {
    this.editorFocusPending = false;
    this.editing = false;
    this.editTarget = '';
    this.draft = '';
  }

  private focus(id: string): void {
    try {
      this.getUIContext().getFocusController().requestFocus(id);
    } catch (error) {
      hilog.warn(0xD001, 'ShinkiEMU', 'focus %{public}s: %{public}s', id, (error as BusinessError).message);
    }
  }

  private finishEditor(): void {
    this.closeEditor();
    this.focus(STAGE_FOCUS);
  }

  private submitText(): void {
    if (this.editTarget !== '' && this.editTarget === this.textTarget && this.draft !== '') {
      manager.submitText(this.editTarget, this.draft);
    }
    this.finishEditor();
  }

  onBackPress(): boolean {
    if (this.editing) {
      this.finishEditor();
      return true;
    }
    manager.pressKey(KEY_BACK, true);
    manager.pressKey(KEY_BACK, false);
    return true;
  }

  build() {
    Stack({ alignContent: Alignment.BottomEnd }) {
      if (this.ready) {
        XComponent({ id: 'shinki-emu-manager', type: XComponentType.SURFACE, libraryname: 'shinki_emu_manager' })
          .width('100%')
          .height('100%')
          .id(STAGE_FOCUS)
          .focusable(true)
          .defaultFocus(true)
        if (this.textTarget !== '') {
          if (this.editing) {
            Row({ space: 8 }) {
              TextInput({ text: this.draft })
                .id(EDITOR_FOCUS)
                .layoutWeight(1)
                .maxLength(8192)
                .onAreaChange(() => {
                  if (this.editorFocusPending) {
                    this.editorFocusPending = false;
                    this.focus(EDITOR_FOCUS);
                  }
                })
                .enterKeyType(EnterKeyType.Done)
                .enableKeyboardOnFocus(true)
                .enablePreviewText(false)
                .onChange((value: string) => { this.draft = value; })
                .onSubmit(() => { this.submitText(); })
              Button($r('app.string.session_text_submit'))
                .onClick(() => { this.submitText(); })
              Button($r('app.string.session_text_cancel'))
                .onClick(() => { this.finishEditor(); })
            }
            .width('100%')
            .padding(8)
            .backgroundColor($r('app.color.start_window_background'))
          } else {
            Button($r('app.string.session_text_input'))
              .margin(8)
              .onClick(() => {
                this.editTarget = this.textTarget;
                this.editorFocusPending = true;
                this.editing = true;
              })
          }
        }
      } else {
        Text(this.failure)
          .fontSize(16)
          .fontColor(Color.White)
          .padding(24)
          .align(Alignment.Center)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#10131A')
  }
}
