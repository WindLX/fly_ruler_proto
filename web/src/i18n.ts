import { createI18n } from 'vue-i18n'

export const messages = {
  'zh-CN': {
    app: {
      title: 'FlyRuler 控制中心',
      aircraft: '飞行器与数据',
      charts: '曲线工作区',
      inspector: '状态与样式',
      sessions: '会话',
      fields: '字段目录',
      live: '实时',
      pause: '暂停',
      play: '播放',
      save: '保存',
      load: '加载',
      clear: '清空内存',
      createChart: '创建图表',
      addSelected: '加入选中图表',
      noData: '暂无数据',
    },
  },
  en: {
    app: {
      title: 'FlyRuler Control Center',
      aircraft: 'Aircraft & Data',
      charts: 'Chart Workspace',
      inspector: 'State & Style',
      sessions: 'Sessions',
      fields: 'Field Catalog',
      live: 'Live',
      pause: 'Pause',
      play: 'Play',
      save: 'Save',
      load: 'Load',
      clear: 'Clear Memory',
      createChart: 'Create Chart',
      addSelected: 'Add to selected chart',
      noData: 'No data',
    },
  },
}

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'en',
  messages,
})
