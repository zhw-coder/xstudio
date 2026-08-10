import { invoke } from '@tauri-apps/api/core';
import type { MouseEvent } from 'react';
import { SettingsPanelHeader } from '../../components';
import { I18n } from '../../i18n';
import { ReportBackendError } from '../../utils/backendError';

/// XStudio 开源仓库地址。
const ProjectSourceUrl = 'https://github.com/zhw-coder/xstudio';

/// XStudio 最新版本下载地址。
const ProjectReleasesUrl = 'https://github.com/zhw-coder/xstudio/releases';

/// 打开外部链接的后端命令名。
const OpenExternalUrlCommand = 'open_external_url';

/// 渲染设置中的项目关于信息。
function AboutSettingsPanel() {
  /// 使用系统默认浏览器打开项目相关链接。
  /// @param event 超链接点击事件。
  /// @param url 需要打开的外部链接。
  async function OpenProjectUrl(event: MouseEvent<HTMLAnchorElement>, url: string) {
    event.preventDefault();

    try {
      await invoke(OpenExternalUrlCommand, { url });
    } catch (error) {
      ReportBackendError(I18n.settings.aboutOpenUrlError, error);
    }
  }

  return (
    <section className="AboutSettingsPanel" aria-labelledby="about-settings-title">
      <SettingsPanelHeader title={I18n.settings.aboutTitle} titleId="about-settings-title" />

      <div className="AboutSettingsContent">
        <p>
          {I18n.settings.aboutIntro}
          <a href={ProjectSourceUrl} onClick={(event) => void OpenProjectUrl(event, ProjectSourceUrl)}>
            {I18n.settings.aboutSourceLink}
          </a>
        </p>
        <p>
          {I18n.settings.aboutDownloadLabel}
          <a href={ProjectReleasesUrl} onClick={(event) => void OpenProjectUrl(event, ProjectReleasesUrl)}>
            {I18n.settings.aboutDownloadLink}
          </a>
        </p>
        <p className="AboutSettingsParagraph">{I18n.settings.aboutParagraph}</p>
        <p>{I18n.settings.aboutNotice}</p>
        <p>{I18n.settings.aboutCommunity}</p>
        <p>
          <strong>{I18n.settings.aboutAuthor}</strong>
        </p>
      </div>
    </section>
  );
}

export default AboutSettingsPanel;
