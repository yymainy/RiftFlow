import { GlobalThemeOverrides } from 'naive-ui'

/** Neutral desktop palette shared by the dark and graphite appearances. */
export const CODEX_THEME_OVERRIDES: GlobalThemeOverrides = {
  common: {
    fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Microsoft YaHei', sans-serif",
    fontSize: '13px',
    fontSizeSmall: '12px',
    borderRadius: '6px',
    borderRadiusSmall: '4px',
    heightSmall: '28px',
    heightMedium: '32px',
    heightLarge: '38px',
    bodyColor: '#202020',
    cardColor: '#252525',
    modalColor: '#252525',
    tableColor: '#252525',
    popoverColor: '#292929',
    primaryColor: '#adc7b6',
    primaryColorHover: '#c1d8c9',
    primaryColorPressed: '#91ad9b',
    primaryColorSuppl: '#adc7b6',
    infoColor: '#adc7b6',
    infoColorHover: '#c1d8c9',
    infoColorPressed: '#91ad9b',
    infoColorSuppl: '#adc7b6',
    successColor: '#9ebba8',
    successColorHover: '#b6cebf',
    successColorPressed: '#839f8d',
    successColorSuppl: '#9ebba8',
    warningColor: '#c4ad84',
    warningColorHover: '#d4bf9a',
    warningColorPressed: '#aa916c',
    warningColorSuppl: '#c4ad84',
    errorColor: '#d49994',
    errorColorHover: '#e0b0ab',
    errorColorPressed: '#b57b76',
    errorColorSuppl: '#d49994',
    textColorBase: '#e7e7e7',
    textColor1: '#e7e7e7',
    textColor2: '#b8b8b8',
    textColor3: '#939393',
    textColorDisabled: '#666666',
    placeholderColor: '#737373',
    placeholderColorDisabled: '#535353',
    iconColor: '#939393',
    iconColorHover: '#e7e7e7',
    iconColorPressed: '#b8b8b8',
    borderColor: '#343434',
    dividerColor: '#343434',
    inputColor: '#222222',
    actionColor: '#292929',
    hoverColor: '#303030',
    pressedColor: '#353535',
    buttonColor2: '#303030',
    buttonColor2Hover: '#3a3a3a',
    buttonColor2Pressed: '#262626',
    boxShadow1: '0 4px 16px #00000024',
    boxShadow2: '0 8px 28px #00000038'
  },
  Card: {
    color: '#252525',
    colorModal: '#252525',
    borderColor: '#343434',
    borderColorModal: '#343434',
    borderRadius: '8px',
    titleFontSizeMedium: '14px',
    titleFontWeight: '600',
    paddingMedium: '16px'
  },
  Dialog: { color: '#252525', borderRadius: '8px', titleFontSize: '16px', titleFontWeight: '600' },
  Modal: { color: '#252525' },
  Notification: {
    color: '#292929',
    padding: '12px',
    titleFontSize: '13px',
    titleFontWeight: '600',
    descriptionFontSize: '12px',
    avatarSize: '20px'
  },
  Message: {
    colorInfo: '#292929',
    colorSuccess: '#292929',
    colorWarning: '#292929',
    colorError: '#292929',
    padding: '8px 12px',
    fontSize: '12px',
    iconSize: '16px'
  },
  Button: {
    borderRadiusMedium: '6px',
    borderRadiusSmall: '5px',
    fontWeight: '500',
    textColorPrimary: '#202820',
    textColorHoverPrimary: '#202820',
    textColorPressedPrimary: '#202820',
    colorSecondary: '#303030',
    colorSecondaryHover: '#3a3a3a',
    colorSecondaryPressed: '#292929'
  },
  Input: {
    color: '#222222',
    colorFocus: '#252525',
    borderHover: '1px solid #626f65',
    boxShadowFocus: '0 0 0 2px #adc7b619'
  },
  Switch: {
    railColor: '#444444',
    railColorActive: '#8eaa98',
    buttonColor: '#eeeeee',
    buttonBoxShadow: 'none'
  },
  Checkbox: {
    colorChecked: '#adc7b6',
    borderChecked: '1px solid #adc7b6',
    checkMarkColor: '#202820',
    fontSizeSmall: '12px'
  },
  Radio: {
    dotColorActive: '#adc7b6',
    buttonBorderColorActive: '#adc7b6',
    buttonTextColorActive: '#c1d8c9',
    boxShadowFocus: '0 0 0 2px #adc7b619'
  },
  Tabs: {
    tabFontSizeMedium: '12px',
    tabFontWeight: '500',
    tabFontWeightActive: '600',
    tabTextColorActiveBar: '#e7e7e7',
    barColor: '#b8c5bc',
    panePaddingMedium: '16px 0 0 0'
  },
  DataTable: {
    thColor: '#292929',
    tdColor: '#252525',
    tdColorHover: '#303030',
    borderColor: '#343434',
    thFontWeight: '500',
    thPaddingMedium: '10px 12px',
    tdPaddingMedium: '9px 12px'
  },
  Popover: {
    color: '#292929',
    arrowColor: '#292929',
    borderColor: '#414141',
    fontSize: '12px',
    borderRadius: '6px'
  },
  Tooltip: { color: '#333333', textColor: '#e7e7e7' },
  InternalSelectMenu: { color: '#292929' },
  Dropdown: { color: '#292929', borderRadius: '6px' },
  Menu: {
    padding: '4px',
    itemColorActive: '#343434',
    itemColorActiveHover: '#3a3a3a',
    itemTextColorActive: '#e7e7e7',
    itemIconColorActive: '#e7e7e7',
    borderRadius: '6px'
  },
  Scrollbar: { width: '6px', color: '#494949', colorHover: '#666666' }
}
