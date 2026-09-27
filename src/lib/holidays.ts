/**
 * 法定节假日与调休上班日（F16 月总览）。
 *
 * 日期格式与全局一致：数字 YYYYMMDD。
 *
 * ⚠️ 数据按年硬编码，来源为国务院办公厅《关于 2026 年部分节假日安排的通知》
 * （2026-09-20 经中国政府网 / 光明网 / 武汉市人民政府等多源交叉核对，放假调休共 33 天）。
 * 国务院通常每年 11 月左右发布次年安排，**届时必须在此增补新的一年**，
 * 否则跨年后月总览只是缺节假日标记（功能降级，不影响其他部分）。
 *
 * 区间比较用数值大小：本文件所有区间都在同一自然月内（数字连续），
 * 若将来要写跨月区间，必须改成逐日展开，否则数值比较会漏掉中间日期。
 */

interface YearConfig {
  /** [起始日, 结束日, 节日名]，区间须落在同一自然月内 */
  holidays: [number, number, string][];
  /** 调休上班日（原本是周末，因调休需上班） */
  workdays: number[];
}

const RAW: Record<number, YearConfig> = {
  2026: {
    holidays: [
      [20260101, 20260103, "元旦"],
      [20260215, 20260223, "春节"],
      [20260404, 20260406, "清明"],
      [20260501, 20260505, "劳动节"],
      [20260619, 20260621, "端午"],
      [20260925, 20260927, "中秋"],
      [20261001, 20261007, "国庆"],
    ],
    workdays: [20260104, 20260214, 20260228, 20260509, 20260920, 20261010],
  },
};

export interface DayMark {
  /** 节日名（仅放假日有） */
  name?: string;
  /** 法定放假日 */
  holiday: boolean;
  /** 调休上班日（周末但需上班） */
  workday: boolean;
}

const EMPTY: DayMark = { holiday: false, workday: false };

/** 查询某日的节假日/调休标记；该年份无数据时一律返回"普通日"。 */
export function dayMark(ymd: number): DayMark {
  const year = Math.floor(ymd / 10000);
  const cfg = RAW[year];
  if (!cfg) return EMPTY;

  for (const [from, to, name] of cfg.holidays) {
    if (ymd >= from && ymd <= to) return { name, holiday: true, workday: false };
  }
  if (cfg.workdays.includes(ymd)) return { holiday: false, workday: true };
  return EMPTY;
}

/** 有数据的年份列表（设置页/自检用）。 */
export function holidayYears(): number[] {
  return Object.keys(RAW).map(Number).sort((a, b) => a - b);
}
