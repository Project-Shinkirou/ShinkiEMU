import { hvigor, HvigorNode } from '@ohos/hvigor';
import { appTasks, OhosAppContext, OhosPluginId } from '@ohos/hvigor-ohos-plugin';

// `cargo xtask package --target aarch64-unknown-linux-ohos --manager` passes the
// release version; plain DevEco builds keep AppScope/app.json5's versionName.
const versionName = process.env.SHINKI_EMU_OHOS_VERSION_NAME;
if (versionName) {
  hvigor.nodesEvaluated(() => {
    const node: HvigorNode = hvigor.getRootNode();
    const context = node.getContext(OhosPluginId.OHOS_APP_PLUGIN) as OhosAppContext;
    const app = context.getAppJsonOpt();
    app['app']['versionName'] = versionName;
    context.setAppJsonOpt(app);
  });
}

export default {
  system: appTasks,
  plugins: []
};
