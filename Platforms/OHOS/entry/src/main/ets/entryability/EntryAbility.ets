import { AbilityConstant, Configuration, UIAbility, Want } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { BusinessError } from '@kit.BasicServicesKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import manager from 'libshinki_emu_manager.so';
import { reportConfiguration } from '../bridge/ManagerBridge';

export default class EntryAbility extends UIAbility {
  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0xD001, 'ShinkiEMU', 'launch reason %{public}d', launchParam.launchReason);
  }

  onWindowStageCreate(stage: window.WindowStage): void {
    stage.loadContent('pages/Index', (error: BusinessError) => {
      if (error.code) {
        hilog.error(0xD001, 'ShinkiEMU', 'loadContent failed: %{public}s', error.message);
      }
    });
  }

  onForeground(): void {
    manager.foreground(true);
  }

  onBackground(): void {
    manager.foreground(false);
  }

  onConfigurationUpdate(configuration: Configuration): void {
    reportConfiguration(configuration.colorMode);
  }
}
