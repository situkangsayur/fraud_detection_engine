// Registers only the ECharts pieces we use (tree-shaken) and the <VChart> component.
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart, HeatmapChart, LineChart, PieChart, ScatterChart } from 'echarts/charts'
import { DataZoomComponent, GridComponent, LegendComponent, MarkLineComponent, TitleComponent, TooltipComponent, VisualMapComponent } from 'echarts/components'
import VChart from 'vue-echarts'

export default defineNuxtPlugin((nuxtApp) => {
  use([CanvasRenderer, BarChart, LineChart, PieChart, ScatterChart, HeatmapChart, GridComponent, TooltipComponent, LegendComponent, TitleComponent, DataZoomComponent, VisualMapComponent, MarkLineComponent])
  nuxtApp.vueApp.component('VChart', VChart)
})
