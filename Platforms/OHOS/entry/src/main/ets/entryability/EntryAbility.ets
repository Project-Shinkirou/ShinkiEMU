import UIAbility from '@ohos.app.ability.UIAbility';
import window from '@ohos.window';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { BusinessError } from '@kit.BasicServicesKit';

export default class EntryAbility extends UIAbility {
  onCreate(): void {
    this.context.filesDir;
  }

  onWindowStageCreate(stage: window.WindowStage): void {
    stage.loadContent('pages/Index', (error: BusinessError) => {
      if (error.code) hilog.error(0xD001, 'ShinkiEMU', 'loadContent failed: %{public}s', error.message);
    });
  }

  onForeground(): void {
    hilog.info(0xD001, 'ShinkiEMU', 'Manager foregrounded');
  }

  onBackground(): void {
    hilog.info(0xD001, 'ShinkiEMU', 'Manager backgrounded');
  }
}
