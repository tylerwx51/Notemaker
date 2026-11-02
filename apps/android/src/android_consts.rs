//! Android `static final` compile-time constants used by the Java sources,
//! with the values from `android.jar` (API 36, read with `javap -constants`).
//! Java inlined these at compile time, so the port uses Rust constants named
//! `CLASS_FIELD` (nested classes joined with `_`). Enum-like objects
//! (`Paint.Style.FILL`, `Path.Op.UNION`, `Bitmap.Config.ARGB_8888`, …) are not
//! constants: read them with `jsget!`.
#![allow(dead_code)]

/// `android.graphics.Color.BLACK`
pub const COLOR_BLACK: i32 = 0xff000000u32 as i32;
/// `android.widget.Toast.LENGTH_LONG`
pub const TOAST_LENGTH_LONG: i32 = 1;
/// `android.graphics.Paint.ANTI_ALIAS_FLAG`
pub const PAINT_ANTI_ALIAS_FLAG: i32 = 1;
/// `android.view.MotionEvent.ACTION_MOVE`
pub const MOTIONEVENT_ACTION_MOVE: i32 = 2;
/// `android.view.MotionEvent.ACTION_DOWN`
pub const MOTIONEVENT_ACTION_DOWN: i32 = 0;
/// `android.view.MotionEvent.ACTION_UP`
pub const MOTIONEVENT_ACTION_UP: i32 = 1;
/// `android.view.MotionEvent.TOOL_TYPE_STYLUS`
pub const MOTIONEVENT_TOOL_TYPE_STYLUS: i32 = 2;
/// `android.graphics.Color.WHITE`
pub const COLOR_WHITE: i32 = 0xffffffffu32 as i32;
/// `android.graphics.Color.TRANSPARENT`
pub const COLOR_TRANSPARENT: i32 = 0;
/// `android.util.Base64.DEFAULT`
pub const BASE64_DEFAULT: i32 = 0;
/// `android.widget.Toast.LENGTH_SHORT`
pub const TOAST_LENGTH_SHORT: i32 = 0;
/// `android.view.InputDevice.SOURCE_STYLUS`
pub const INPUTDEVICE_SOURCE_STYLUS: i32 = 16386;
/// `android.view.Gravity.CENTER_VERTICAL`
pub const GRAVITY_CENTER_VERTICAL: i32 = 16;
/// `android.view.Gravity.CENTER`
pub const GRAVITY_CENTER: i32 = 17;
/// `android.graphics.Color.RED`
pub const COLOR_RED: i32 = 0xffff0000u32 as i32;
/// `android.content.DialogInterface.BUTTON_POSITIVE`
pub const DIALOGINTERFACE_BUTTON_POSITIVE: i32 = -1;
/// `android.view.View.VISIBLE`
pub const VIEW_VISIBLE: i32 = 0;
/// `android.view.View.GONE`
pub const VIEW_GONE: i32 = 8;
/// `android.view.MotionEvent.ACTION_CANCEL`
pub const MOTIONEVENT_ACTION_CANCEL: i32 = 3;
/// `android.widget.LinearLayout.VERTICAL`
pub const LINEARLAYOUT_VERTICAL: i32 = 1;
/// `android.text.InputType.TYPE_CLASS_TEXT`
pub const INPUTTYPE_TYPE_CLASS_TEXT: i32 = 1;
/// `android.content.DialogInterface.BUTTON_NEUTRAL`
pub const DIALOGINTERFACE_BUTTON_NEUTRAL: i32 = -3;
/// `android.view.MotionEvent.TOOL_TYPE_FINGER`
pub const MOTIONEVENT_TOOL_TYPE_FINGER: i32 = 1;
/// `android.content.Intent.CATEGORY_OPENABLE`
pub const INTENT_CATEGORY_OPENABLE: &str = "android.intent.category.OPENABLE";
/// `android.content.Intent.ACTION_OPEN_DOCUMENT`
pub const INTENT_ACTION_OPEN_DOCUMENT: &str = "android.intent.action.OPEN_DOCUMENT";
/// `android.content.DialogInterface.BUTTON_NEGATIVE`
pub const DIALOGINTERFACE_BUTTON_NEGATIVE: i32 = -2;
/// `android.graphics.Typeface.ITALIC`
pub const TYPEFACE_ITALIC: i32 = 2;
/// `android.graphics.Typeface.BOLD`
pub const TYPEFACE_BOLD: i32 = 1;
/// `android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT`
pub const MOTIONEVENT_ACTION_POINTER_INDEX_SHIFT: i32 = 8;
/// `android.view.MotionEvent.ACTION_POINTER_DOWN`
pub const MOTIONEVENT_ACTION_POINTER_DOWN: i32 = 5;
/// `android.widget.LinearLayout.HORIZONTAL`
pub const LINEARLAYOUT_HORIZONTAL: i32 = 0;
/// `android.text.InputType.TYPE_TEXT_VARIATION_URI`
pub const INPUTTYPE_TYPE_TEXT_VARIATION_URI: i32 = 16;
/// `android.view.InputDevice.SOURCE_TOUCHSCREEN`
pub const INPUTDEVICE_SOURCE_TOUCHSCREEN: i32 = 4098;
/// `android.view.Gravity.TOP`
pub const GRAVITY_TOP: i32 = 48;
/// `android.view.FrameMetrics.TOTAL_DURATION`
pub const FRAMEMETRICS_TOTAL_DURATION: i32 = 8;
/// `android.view.FrameMetrics.DRAW_DURATION`
pub const FRAMEMETRICS_DRAW_DURATION: i32 = 4;
/// `android.view.FrameMetrics.COMMAND_ISSUE_DURATION`
pub const FRAMEMETRICS_COMMAND_ISSUE_DURATION: i32 = 6;
/// `android.graphics.Color.LTGRAY`
pub const COLOR_LTGRAY: i32 = 0xffccccccu32 as i32;
/// `android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE`
pub const WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_RESIZE: i32 = 16;
/// `android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING`
pub const WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_NOTHING: i32 = 48;
/// `android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND`
pub const WM_LAYOUT_PARAMS_FLAG_DIM_BEHIND: i32 = 2;
/// `android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS`
pub const WINDOWINSETSCONTROLLER_APPEARANCE_LIGHT_STATUS_BARS: i32 = 8;
/// `android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS`
pub const WINDOWINSETSCONTROLLER_APPEARANCE_LIGHT_NAVIGATION_BARS: i32 = 16;
/// `android.view.View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`
pub const VIEW_SYSTEM_UI_FLAG_LIGHT_STATUS_BAR: i32 = 8192;
/// `android.view.View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR`
pub const VIEW_SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR: i32 = 16;
/// `android.view.View.MeasureSpec.UNSPECIFIED`
pub const MEASURE_SPEC_UNSPECIFIED: i32 = 0;
/// `android.view.View.MeasureSpec.EXACTLY`
pub const MEASURE_SPEC_EXACTLY: i32 = 0x40000000;
/// `android.view.ViewGroup.LayoutParams.MATCH_PARENT`
pub const LAYOUT_PARAMS_MATCH_PARENT: i32 = -1;
/// `android.view.ViewGroup.LayoutParams.WRAP_CONTENT`
pub const LAYOUT_PARAMS_WRAP_CONTENT: i32 = -2;
/// `android.view.View.FOCUS_DOWN`
pub const VIEW_FOCUS_DOWN: i32 = 130;
/// `android.graphics.Typeface.NORMAL`
pub const TYPEFACE_NORMAL: i32 = 0;
/// `android.graphics.Paint.FILTER_BITMAP_FLAG`
pub const PAINT_FILTER_BITMAP_FLAG: i32 = 2;
/// `android.graphics.Paint.DITHER_FLAG`
pub const PAINT_DITHER_FLAG: i32 = 4;
/// `android.view.MotionEvent.TOOL_TYPE_ERASER`
pub const MOTIONEVENT_TOOL_TYPE_ERASER: i32 = 4;
/// `android.view.MotionEvent.ACTION_POINTER_UP`
pub const MOTIONEVENT_ACTION_POINTER_UP: i32 = 6;
/// `android.view.MotionEvent.ACTION_MASK`
pub const MOTIONEVENT_ACTION_MASK: i32 = 255;
/// `android.content.Intent.FLAG_ACTIVITY_NEW_TASK`
pub const INTENT_FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;
/// `android.content.Intent.EXTRA_MIME_TYPES`
pub const INTENT_EXTRA_MIME_TYPES: &str = "android.intent.extra.MIME_TYPES";
/// `android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD`
pub const INPUTTYPE_TYPE_TEXT_VARIATION_PASSWORD: i32 = 128;
/// `android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE`
pub const INPUTTYPE_TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x20000;
/// `android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES`
pub const INPUTTYPE_TYPE_TEXT_FLAG_CAP_SENTENCES: i32 = 16384;
/// `android.text.InputType.TYPE_CLASS_NUMBER`
pub const INPUTTYPE_TYPE_CLASS_NUMBER: i32 = 2;
/// `android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL`
pub const INPUTTYPE_TYPE_NUMBER_FLAG_DECIMAL: i32 = 8192;
/// `android.text.InputType.TYPE_NUMBER_FLAG_SIGNED`
pub const INPUTTYPE_TYPE_NUMBER_FLAG_SIGNED: i32 = 4096;
/// `android.widget.GridView.STRETCH_COLUMN_WIDTH`
pub const GRIDVIEW_STRETCH_COLUMN_WIDTH: i32 = 2;
/// `android.widget.GridView.AUTO_FIT`
pub const GRIDVIEW_AUTO_FIT: i32 = -1;
/// `android.view.Gravity.RIGHT`
pub const GRAVITY_RIGHT: i32 = 5;
/// `android.view.Gravity.LEFT`
pub const GRAVITY_LEFT: i32 = 3;
/// `android.view.Gravity.CENTER_HORIZONTAL`
pub const GRAVITY_CENTER_HORIZONTAL: i32 = 1;
/// `android.view.FrameMetrics.UNKNOWN_DELAY_DURATION`
pub const FRAMEMETRICS_UNKNOWN_DELAY_DURATION: i32 = 0;
/// `android.view.FrameMetrics.SYNC_DURATION`
pub const FRAMEMETRICS_SYNC_DURATION: i32 = 5;
/// `android.view.FrameMetrics.SWAP_BUFFERS_DURATION`
pub const FRAMEMETRICS_SWAP_BUFFERS_DURATION: i32 = 7;
/// `android.view.FrameMetrics.LAYOUT_MEASURE_DURATION`
pub const FRAMEMETRICS_LAYOUT_MEASURE_DURATION: i32 = 3;
/// `android.view.FrameMetrics.INPUT_HANDLING_DURATION`
pub const FRAMEMETRICS_INPUT_HANDLING_DURATION: i32 = 1;
/// `android.view.FrameMetrics.ANIMATION_DURATION`
pub const FRAMEMETRICS_ANIMATION_DURATION: i32 = 2;
/// `android.graphics.Color.BLUE`
pub const COLOR_BLUE: i32 = 0xff0000ffu32 as i32;
/// `android.app.Activity.RESULT_OK`
pub const ACTIVITY_RESULT_OK: i32 = -1;
/// `android.app.Activity.RESULT_CANCELED`
pub const ACTIVITY_RESULT_CANCELED: i32 = 0;
/// `android.app.Service.START_NOT_STICKY`
pub const SERVICE_START_NOT_STICKY: i32 = 2;
/// `android.content.Context.MODE_PRIVATE`
pub const CONTEXT_MODE_PRIVATE: i32 = 0;
/// `android.R.layout.simple_spinner_dropdown_item`
pub const R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM: i32 = 0x1090009;
/// `android.os.Build.VERSION_CODES.R`
pub const VERSION_CODES_R: i32 = 30;
