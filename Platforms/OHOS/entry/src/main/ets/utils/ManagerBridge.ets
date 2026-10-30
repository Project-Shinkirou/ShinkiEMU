import hilog from '@ohos.hilog';
import fileIo from '@ohos.file.fs';
import { napi } from '@kit.ArkTS';
import { filePicker } from '@kit.CoreFileKit';

export function onEvent(kind: string, payload: string): void {
  if (kind === 'error') hilog.error(0xD001, 'ShinkiEMU', '%{public}s', payload);
  else hilog.info(0xD001, 'ShinkiEMU', '%{public}s %{public}s', kind, payload);
}

export async function selectFile(kind: string, title: string, extensions: string[]): Promise<string | undefined> {
  if (kind === 'pickFolder' || kind === 'pickDirectory') {
    const result = await new filePicker.FolderPicker().select();
    return result.length ? result[0] : undefined;
  }
  const options = new filePicker.DocumentSelectOptions();
  options.maxSelectNumber = 1;
  if (extensions.length) options.fileSuffixFilters = extensions.map((ext: string) => `.${ext.replace(/^\\./, '')}`);
  const result = await new filePicker.DocumentViewPicker().select(options);
  return result.length ? result[0] : undefined;
}

export function loadNative(context: object): void {
  const native = requireNapi('shinki_emu_manager', false);
  native.setEventListener((kind: string, payload: string) => {
    if (kind === 'pickFolder' || kind === 'pickFile' || kind === 'pickDirectory') {
      const request = JSON.parse(payload) as { id: number, title: string, extensions: string[] };
      selectFile(kind, request.title, request.extensions).then((path: string | undefined) => {
        native.completePicker(request.id, path ?? '', path ? '' : 'cancelled');
      }).catch((error: Error) => native.completePicker(request.id, '', error.message));
    } else onEvent(kind, payload);
  });
  native.start((context as { filesDir: string }).filesDir);
}

export function reportPickerFailure(id: number, message: string): void {
  hilog.error(0xD001, 'ShinkiEMU', 'picker %d failed: %{public}s', id, message);
}
