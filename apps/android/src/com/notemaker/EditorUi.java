package com.notemaker;

import android.app.AlertDialog;
import android.content.res.ColorStateList;
import android.graphics.*;
import android.graphics.drawable.GradientDrawable;
import android.text.TextUtils;
import android.view.*;
import android.widget.*;
import org.json.JSONObject;
import java.util.*;

/** Editor chrome. The layer tray overlays the canvas so toggling it never moves ink. */
final class EditorUi {
    private static final int INK = 0xff202521;
    private static final int MUTED = 0xff636b65;
    private static final int LINE = 0xffd8ddd7;
    private static final int PAPER = 0xfff5f6f2;
    private final MainActivity app;
    private final Map<String, Tool> tools = new LinkedHashMap<>();
    private final ArrayList<LayerPreview> previews = new ArrayList<>();
    private LinearLayout root, tray, layerList;
    private TextView title, subtitle, connection;
    private FrameLayout workspace;
    private PopupWindow popup;
    private boolean expanded;
    private String trayState = "";
    private long thumbnailTime;
    private int thumbnailRevision = -1;

    EditorUi(MainActivity app) {
        this.app = app;
        expanded = app.getSharedPreferences("settings", 0).getBoolean("layerTrayExpanded", true);
    }

    int dp(float value) { return Math.round(value * app.getResources().getDisplayMetrics().density); }

    View build() {
        root = column();
        root.setBackgroundColor(Color.WHITE);
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            view.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                    insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            return insets;
        });

        LinearLayout header = row();
        header.setGravity(Gravity.CENTER_VERTICAL);
        header.setPadding(dp(22), 0, dp(24), 0);
        TextView wordmark = text("NOTE / MAKER", 12, INK, true);
        wordmark.setLetterSpacing(.13f);
        header.addView(wordmark, new LinearLayout.LayoutParams(dp(154), -2));
        LinearLayout heading = column();
        title = text("", 18, INK, true);
        title.setSingleLine(true);
        title.setEllipsize(TextUtils.TruncateAt.END);
        subtitle = text("", 11, MUTED, false);
        heading.addView(title);
        heading.addView(subtitle);
        header.addView(heading, new LinearLayout.LayoutParams(0, -2, 1));
        connection = text("", 12, MUTED, false);
        connection.setGravity(Gravity.RIGHT);
        header.addView(connection);
        header.addView(action("Agent", () -> { dismissPopup(); app.agent.show(); }));
        root.addView(header, new LinearLayout.LayoutParams(-1, dp(70)));
        root.addView(divider(), new LinearLayout.LayoutParams(-1, dp(1)));

        LinearLayout body = row();
        root.addView(body, new LinearLayout.LayoutParams(-1, 0, 1));
        LinearLayout rail = column();
        rail.setGravity(Gravity.CENTER_HORIZONTAL);
        rail.setPadding(dp(8), dp(14), dp(8), dp(12));
        rail.setBackgroundColor(PAPER);
        body.addView(rail, new LinearLayout.LayoutParams(dp(76), -1));
        tool(rail, "document", "Notes", "Document actions");
        space(rail, 18);
        tool(rail, "pen", "Pen", "Pen tools");
        tool(rail, "eraser", "Erase", "Eraser tools");
        tool(rail, "move", "Move", "Canvas navigation");
        space(rail, 18);
        tool(rail, "history", "History", "Undo and redo");
        rail.addView(new View(app), new LinearLayout.LayoutParams(1, 0, 1));
        tool(rail, "page", "Pages", "Page actions");
        body.addView(divider(), new LinearLayout.LayoutParams(dp(1), -1));

        workspace = new FrameLayout(app);
        body.addView(workspace, new LinearLayout.LayoutParams(0, -1, 1));
        app.canvas = app.new CanvasView(app);
        app.canvas.setContentDescription("Drawing canvas");
        workspace.addView(app.canvas, new FrameLayout.LayoutParams(-1, -1));
        buildTray();
        return root;
    }

    private LinearLayout column() { LinearLayout v = new LinearLayout(app); v.setOrientation(1); return v; }
    private LinearLayout row() { LinearLayout v = new LinearLayout(app); v.setOrientation(0); return v; }
    private View divider() { View v = new View(app); v.setBackgroundColor(LINE); return v; }
    private void space(LinearLayout parent, int height) { parent.addView(new View(app), new LinearLayout.LayoutParams(1, dp(height))); }
    private TextView text(String value, int size, int color, boolean bold) {
        TextView view = new TextView(app);
        view.setText(value);
        view.setTextSize(size);
        view.setTextColor(color);
        if (bold) view.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        return view;
    }
    private GradientDrawable surface(int fill, int border, int radius, int stroke) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(fill);
        shape.setCornerRadius(dp(radius));
        if (stroke > 0) shape.setStroke(dp(stroke), border);
        return shape;
    }
    private TextView action(String label, Runnable callback) {
        TextView button = text(label, 14, INK, true);
        button.setGravity(Gravity.CENTER_VERTICAL);
        button.setPadding(dp(14), dp(12), dp(14), dp(12));
        button.setMinHeight(dp(48));
        button.setBackground(surface(PAPER, LINE, 8, 1));
        button.setContentDescription(label);
        button.setOnClickListener(v -> callback.run());
        return button;
    }
    private void menuAction(LinearLayout parent, String label, boolean enabled, Runnable callback) {
        TextView button = action(label, () -> { dismissPopup(); callback.run(); });
        button.setEnabled(enabled);
        button.setAlpha(enabled ? 1f : .4f);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, -2);
        params.topMargin = dp(8);
        parent.addView(button, params);
    }
    private void tool(LinearLayout rail, String icon, String label, String description) {
        Tool tool = new Tool(icon, label, description);
        tools.put(icon, tool);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, dp(66));
        params.bottomMargin = dp(7);
        rail.addView(tool, params);
        tool.setOnClickListener(v -> openTool(icon, tool));
    }

    private void setText(TextView view, String value) {
        if (!value.contentEquals(view.getText())) view.setText(value);
    }

    void refresh() {
        setText(title, app.document);
        setText(subtitle, app.type.equals("pages") ? "NOTEBOOK  /  PAGE " + (app.page + 1)
                : app.type.equals("infinite") ? "INFINITE CANVAS" : "SINGLE PAGE");
        setText(connection, app.saveError!=null ? "Save failed" : app.saving>0 ? (app.replica.remote()?"Saving to proxy…":"Saving on device…") : app.connected
                ? app.pendingCount == 0 ? "●  Connected" : "●  Syncing changes"
                : app.replica.remote()?"○  Proxy unavailable · last snapshot":"○  Offline · saved on device");
        String mode = app.erase ? "eraser" : app.pan ? "move" : "pen";
        for (Map.Entry<String, Tool> entry : tools.entrySet()) entry.getValue().active(entry.getKey().equals(mode));

        StringBuilder state = new StringBuilder(app.document).append('/').append(app.page).append('/').append(app.selected);
        for (Map.Entry<String, JSONObject> entry : app.layers.entrySet()) {
            state.append('|').append(entry.getKey()).append(entry.getValue().toString()).append(app.hidden.contains(entry.getKey())).append(app.model.layerActive.get(entry.getKey()));
        }
        if (!state.toString().equals(trayState)) {
            trayState = state.toString();
            rebuildLayers();
        }
        int revision = app.eventCount + app.pendingCount;
        long now = android.os.SystemClock.uptimeMillis();
        if (revision != thumbnailRevision && (app.canvas.points == null || now - thumbnailTime > 400)) {
            thumbnailRevision = revision;
            thumbnailTime = now;
            for (LayerPreview preview : previews) preview.invalidate();
        }
    }

    private void buildTray() {
        if (tray != null) workspace.removeView(tray);
        tray = column();
        tray.setClickable(true);
        tray.setPadding(dp(8), dp(12), dp(8), dp(12));
        tray.setBackground(surface(PAPER, LINE, 0, 1));
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(dp(expanded ? 248 : 64), -1, Gravity.RIGHT);
        workspace.addView(tray, params);
        LinearLayout heading = row();
        heading.setGravity(Gravity.CENTER_VERTICAL);
        if (expanded) {
            TextView label = text("LAYERS", 12, INK, true);
            label.setLetterSpacing(.14f);
            label.setPadding(dp(8), 0, 0, 0);
            heading.addView(label, new LinearLayout.LayoutParams(0, -2, 1));
        }
        Icon toggle = icon(expanded ? "collapse" : "expand", expanded ? "Collapse layer tray" : "Expand layer tray", () -> {
            dismissPopup();
            expanded = !expanded;
            app.getSharedPreferences("settings", 0).edit().putBoolean("layerTrayExpanded", expanded).apply();
            buildTray();
        });
        heading.addView(toggle, new LinearLayout.LayoutParams(dp(48), dp(48)));
        tray.addView(heading);
        if (expanded) {
            TextView hint = text("Top to bottom", 11, MUTED, false);
            hint.setPadding(dp(8), 0, 0, dp(14));
            tray.addView(hint);
        }
        ScrollView scroll = new ScrollView(app);
        scroll.setFillViewport(false);
        scroll.setVerticalScrollBarEnabled(false);
        layerList = column();
        scroll.addView(layerList);
        tray.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        if (expanded) tray.addView(action("＋  New layer", this::newLayer));
        else tray.addView(icon("plus", "New layer", this::newLayer), new LinearLayout.LayoutParams(-1, dp(48)));
        rebuildLayers();
    }

    private void rebuildLayers() {
        if (layerList == null) return;
        synchronized (app.lock) {
            layerList.removeAllViews();
            previews.clear();
            ArrayList<String> ids = new ArrayList<>(app.layers.keySet());
            ids.removeIf(id -> Boolean.FALSE.equals(app.model.layerActive.get(id)));
            Collections.reverse(ids);
            for (int index = 0; index < ids.size(); index++) {
                String id = ids.get(index);
                JSONObject layer = app.layers.get(id);
                boolean selected = app.selected.equals(id);
                boolean forced = layer.optBoolean("force");
                boolean visible = forced || !app.hidden.contains(id);
                LinearLayout card = expanded ? row() : column();
                card.setGravity(Gravity.CENTER_VERTICAL);
                card.setPadding(dp(6), dp(8), dp(6), dp(8));
                card.setBackground(surface(Color.WHITE, selected ? INK : LINE, 10, selected ? 2 : 1));
                card.setContentDescription("Select layer " + layer.optString("name"));
                card.setOnClickListener(v -> { synchronized (app.lock) { app.selected = id; app.save(); app.update(); } });
                card.setOnLongClickListener(v -> { layerOptions(id, card); return true; });
                LinearLayout.LayoutParams cardParams = new LinearLayout.LayoutParams(-1, -2);
                cardParams.bottomMargin = dp(10);
                layerList.addView(card, cardParams);

                LayerPreview preview = new LayerPreview(id, ids.size() - index, visible);
                previews.add(preview);
                card.addView(preview, new LinearLayout.LayoutParams(dp(expanded ? 48 : 34), dp(expanded ? 70 : 46)));
                if (expanded) {
                    LinearLayout details = column();
                    details.setPadding(dp(10), 0, dp(2), 0);
                    TextView name = text(layer.optString("name"), 13, INK, true);
                    name.setMaxLines(2);
                    name.setEllipsize(TextUtils.TruncateAt.END);
                    details.addView(name);
                    TextView state = text(forced ? "For everyone" : visible ? "Visible" : "Hidden", 11, MUTED, false);
                    state.setPadding(0, dp(5), 0, 0);
                    details.addView(state);
                    if (selected) {
                        TextView active = text("DRAWING LAYER", 9, INK, true);
                        active.setPadding(0, dp(6), 0, 0);
                        details.addView(active);
                    }
                    card.addView(details, new LinearLayout.LayoutParams(0, -2, 1));
                }
                LinearLayout controls = column();
                Icon eye = icon(forced ? "lock" : visible ? "eye" : "hidden",
                        (forced ? "Forced visible layer " : visible ? "Hide layer " : "Show layer ") + layer.optString("name"), () -> {
                            synchronized (app.lock) {
                                if (app.layers.get(id).optBoolean("force")) {
                                    layerOptions(id, card);
                                } else {
                                    if (app.hidden.contains(id)) app.hidden.remove(id); else app.hidden.add(id);
                                    app.save();
                                    app.update();
                                }
                            }
                        });
                controls.addView(eye, new LinearLayout.LayoutParams(dp(expanded ? 36 : 34), dp(38)));
                if (expanded) controls.addView(icon("more", "Options for layer " + layer.optString("name"), () -> layerOptions(id, card)), new LinearLayout.LayoutParams(dp(36), dp(34)));
                card.addView(controls);
            }
        }
    }

    private void newLayer() {
        EditText input = new EditText(app);
        input.setSingleLine(true);
        input.setHint("Layer name");
        LinearLayout box = column();
        box.setPadding(dp(24), dp(8), dp(24), dp(8));
        box.addView(input);
        new AlertDialog.Builder(app).setTitle("New layer").setView(box)
                .setNegativeButton("Cancel", null).setPositiveButton("Create", (dialog, which) -> {
                    String name = input.getText().toString().trim();
                    if (name.isEmpty()) name = "Layer " + (app.layers.size() + 1);
                    synchronized (app.lock) {
                        app.selected = MainActivity.id();
                        app.enqueue(MainActivity.obj("kind", "layer", "layer", app.selected, "name", name, "force", false));
                        app.save();app.update();
                    }
                }).show();
    }

    private void layerOptions(String id, View anchor) {
        JSONObject layer = app.layers.get(id);
        if (layer == null) return;
        LinearLayout box = popupBody(layer.optString("name"), "Layer actions");
        boolean forced = layer.optBoolean("force");
        menuAction(box, forced ? "Stop forcing visibility" : "Force visible for everyone", true, () -> {
            synchronized (app.lock) {
                JSONObject current = app.layers.get(id);
                app.enqueue(MainActivity.obj("kind", "layer", "layer", id, "name", current.optString("name"), "force", !current.optBoolean("force")));
                app.update();
            }
        });
        menuAction(box, "Draw on this layer", true, () -> {
            synchronized (app.lock) { app.selected = id; app.hidden.remove(id); app.save(); app.update(); }
        });
        showPopup(box, anchor, true);
    }

    private LinearLayout popupBody(String heading, String description) {
        LinearLayout box = column();
        box.setPadding(dp(20), dp(20), dp(20), dp(20));
        box.setBackground(surface(Color.WHITE, LINE, 14, 1));
        box.addView(text(heading, 20, INK, true));
        if (!description.isEmpty()) {
            TextView help = text(description, 12, MUTED, false);
            help.setPadding(0, dp(6), 0, dp(10));
            box.addView(help);
        }
        return box;
    }

    private void openTool(String kind, View anchor) {
        LinearLayout box;
        switch (kind) {
            case "pen":
                app.erase = false; app.pan = false; app.update();
                box = popupBody("Pen", "Pressure-sensitive ink");
                LinearLayout swatches = row();
                int[] colors = {Color.BLACK, 0xffbd2424, 0xff245dbd, 0xff247d38, 0xff7824aa, 0xffd06a00};
                String[] names = {"Black", "Red", "Blue", "Green", "Purple", "Orange"};
                for (int i = 0; i < colors.length; i++) {
                    final int color = colors[i];
                    View swatch = new View(app) {
                        final Paint p = new Paint(Paint.ANTI_ALIAS_FLAG);
                        protected void onDraw(Canvas c) {
                            p.setStyle(Paint.Style.FILL); p.setColor(color);
                            c.drawCircle(getWidth()/2f, getHeight()/2f, dp(11), p);
                            if (app.color == color) {
                                p.setStyle(Paint.Style.STROKE); p.setStrokeWidth(dp(1.5f)); p.setColor(INK);
                                c.drawCircle(getWidth()/2f, getHeight()/2f, dp(16), p);
                            }
                        }
                    };
                    swatch.setContentDescription(names[i] + " ink");
                    swatch.setOnClickListener(v -> { app.color = color; app.update(); for (int j = 0; j < swatches.getChildCount(); j++) swatches.getChildAt(j).invalidate(); });
                    swatches.addView(swatch, new LinearLayout.LayoutParams(0, dp(48), 1));
                }
                box.addView(swatches);
                TextView widthLabel = text("Width  " + (int) app.width, 13, INK, true);
                widthLabel.setPadding(0, dp(12), 0, dp(4));
                box.addView(widthLabel);
                SeekBar width = new SeekBar(app);
                width.setContentDescription("Pen width");
                width.setMax(29); width.setProgress((int) app.width - 1);
                width.setProgressTintList(ColorStateList.valueOf(INK));
                width.setThumbTintList(ColorStateList.valueOf(INK));
                width.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
                    public void onProgressChanged(SeekBar bar, int value, boolean user) { app.width = value + 1; widthLabel.setText("Width  " + (value + 1)); }
                    public void onStartTrackingTouch(SeekBar bar) {}
                    public void onStopTrackingTouch(SeekBar bar) {}
                });
                box.addView(width, new LinearLayout.LayoutParams(-1, dp(48)));
                menuAction(box, "Done", true, () -> {});
                break;
            case "eraser":
                app.erase = true; app.pan = false; app.update();
                box = popupBody("Stroke eraser", "Tap or sweep across a stroke to remove it from the selected layer.");
                menuAction(box, "Undo last action", !app.undo.isEmpty(), () -> app.history(false));
                menuAction(box, "Ready to erase", true, () -> {});
                break;
            case "move":
                app.pan = true; app.erase = false; app.update();
                box = popupBody("Move canvas", "Drag to move. Pinch to zoom.");
                menuAction(box, "Reset view", true, () -> { app.canvas.zoom = 1; app.canvas.offsetX = app.canvas.offsetY = 0; app.canvas.invalidate(); });
                menuAction(box, "Return to pen", true, () -> { app.pan = app.erase = false; app.update(); });
                break;
            case "history":
                box = popupBody("History", "Your drawing and erasing actions");
                menuAction(box, "Undo", !app.undo.isEmpty(), () -> app.history(false));
                menuAction(box, "Redo", !app.redo.isEmpty(), () -> app.history(true));
                break;
            case "page":
                box = popupBody(app.type.equals("pages") ? "Page " + (app.page + 1) : app.type.equals("infinite") ? "Infinite canvas" : "Single page", app.type.equals("pages") ? "Navigate your notebook" : "This document uses one continuous drawing surface.");
                if (app.type.equals("pages")) {
                    menuAction(box, "Previous page", app.page > 0, () -> changePage(app.page - 1));
                    menuAction(box, "Next page", app.page < 10000, () -> changePage(app.page + 1));
                    menuAction(box, "Go to page…", true, app::pageDialog);
                }
                menuAction(box, "Reset view", true, () -> { app.canvas.zoom = 1; app.canvas.offsetX = app.canvas.offsetY = 0; app.canvas.invalidate(); });
                break;
            default:
                box = popupBody("Documents", "Open a note or start a new one.");
                menuAction(box, "Open or create document…", true, app::documentDialog);
                menuAction(box, "Edit diagram objects…", !app.model.objects.isEmpty(), app::objectDialog);
                TextView server = text("Server\n" + app.host, 11, MUTED, false);
                server.setPadding(0, dp(18), 0, 0);
                box.addView(server);
                break;
        }
        showPopup(box, anchor, false);
    }

    private void changePage(int page) { synchronized (app.lock) { app.page = page; app.save(); app.update(); } }

    void dismissPopup() { if (popup != null) { popup.dismiss(); popup = null; } }
    private void showPopup(LinearLayout body, View anchor, boolean fromRight) {
        dismissPopup();
        int popupWidth = Math.min(dp(300), root.getWidth() - dp(32));
        body.measure(View.MeasureSpec.makeMeasureSpec(popupWidth, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
        popup = new PopupWindow(body, popupWidth, -2, true);
        popup.setBackgroundDrawable(surface(Color.WHITE, LINE, 14, 1));
        popup.setOutsideTouchable(true);
        popup.setElevation(dp(4));
        popup.setAnimationStyle(0);
        int[] position = new int[2]; anchor.getLocationOnScreen(position);
        int[] rootPosition = new int[2]; root.getLocationOnScreen(rootPosition);
        int x = fromRight ? position[0] - popupWidth - dp(12) : position[0] + anchor.getWidth() + dp(18);
        x = Math.max(dp(12), Math.min(x, root.getWidth() - popupWidth - dp(12)));
        int y = Math.max(rootPosition[1] + root.getPaddingTop() + dp(8), Math.min(position[1], rootPosition[1] + root.getHeight() - root.getPaddingBottom() - body.getMeasuredHeight() - dp(12)));
        popup.showAtLocation(root, Gravity.TOP | Gravity.LEFT, x, y);
    }

    private Icon icon(String kind, String description, Runnable callback) {
        Icon icon = new Icon(kind);
        icon.setContentDescription(description);
        icon.setOnClickListener(v -> callback.run());
        return icon;
    }

    private final class Tool extends LinearLayout {
        final Icon icon;
        final TextView label;
        boolean selected;
        Tool(String kind, String name, String description) {
            super(app);
            setOrientation(VERTICAL); setGravity(Gravity.CENTER);
            setContentDescription(description);
            icon = new Icon(kind);
            label = text(name, 10, INK, true);
            label.setGravity(Gravity.CENTER);
            addView(icon, new LinearLayout.LayoutParams(dp(32), dp(32)));
            addView(label);
            setBackground(surface(PAPER, PAPER, 10, 0));
        }
        void active(boolean value) {
            if (selected == value) return;
            selected = value;
            setBackground(surface(value ? INK : PAPER, LINE, 10, 0));
            icon.color = value ? Color.WHITE : INK;
            icon.invalidate();
            label.setTextColor(value ? Color.WHITE : INK);
        }
    }

    private final class LayerPreview extends View {
        final String layer;
        final int number;
        final boolean visible;
        final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        InkCache raster;String rasterKey="";
        LayerPreview(String layer, int number, boolean visible) { super(app); this.layer = layer; this.number = number; this.visible = visible; }
        protected void onDraw(Canvas c) {
            float w = getWidth(), h = getHeight();
            paint.setStyle(Paint.Style.FILL); paint.setColor(visible ? Color.WHITE : 0xffecefe9);
            c.drawRect(1, 1, w - 1, h - 1, paint);
            float scale = Math.min((w - dp(6)) / 1000f, (h - dp(20)) / 1400f);
            String key=app.document+"/"+app.page+"/"+getWidth()+"/"+getHeight();
            if(!key.equals(rasterKey)){if(raster!=null)raster.bitmap.recycle();raster=new InkCache(getWidth(),getHeight());rasterKey=key;}
            LinkedHashMap<String,JSONObject> settled=new LinkedHashMap<>();
            for(Map.Entry<String,JSONObject> entry:app.strokes.entrySet()){
                JSONObject stroke=entry.getValue();if(!entry.getKey().equals(app.canvas.current)&&layer.equals(stroke.optString("layer"))&&stroke.optInt("page")==app.page&&!Boolean.FALSE.equals(app.active.get(entry.getKey())))settled.put(entry.getKey(),stroke);
            }
            raster.draw(c,settled,image->{image.clipRect(1,dp(16),w-1,h-1);image.translate((w-1000*scale)/2,dp(16));image.scale(scale,scale);},(image,stroke)->{
                JSONObject preview=MainActivity.obj("color",stroke.optInt("color"),"width",Math.max(stroke.optDouble("width"),dp(1.4f)/scale),"points",stroke.optJSONArray("points"));app.canvas.drawStroke(image,preview);
            });
            c.save();c.clipRect(1,dp(16),w-1,h-1);c.translate((w-1000*scale)/2,dp(16));c.scale(scale,scale);
            if(layer.equals(app.selected)&&app.canvas.points!=null)app.canvas.drawStroke(c,app.canvas.stroke());
            synchronized(app.lock){for(Map.Entry<String,Map<String,Object>> entry:app.model.objects.entrySet()){Map<String,Object> o=entry.getValue();if(layer.equals(o.get("layer"))&&((Number)o.get("page")).intValue()==app.page&&!Boolean.FALSE.equals(app.model.objectActive.get(entry.getKey())))com.notemaker.core.Render.drawObject(app.model,o,new com.notemaker.cli.Png(c));}}
            c.restore();
            paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(dp(1)); paint.setColor(LINE);
            c.drawRect(1, 1, w - 1, h - 1, paint);
            if (!visible) { paint.setColor(MUTED); c.drawLine(dp(4), h - dp(4), w - dp(4), dp(4), paint); }
            paint.setStyle(Paint.Style.FILL); paint.setColor(PAPER); c.drawRect(1, 1, dp(17), dp(17), paint);
            paint.setColor(MUTED); paint.setTextSize(dp(10)); c.drawText("" + number, dp(5), dp(12), paint);
        }
    }

    /** Small vector icons stay crisp and monochrome on E Ink. */
    private final class Icon extends View {
        final String kind;
        int color = INK;
        final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        Icon(String kind) { super(app); this.kind = kind; }
        protected void onDraw(Canvas canvas) {
            canvas.save();
            float size = Math.min(getWidth(), getHeight()) * .62f;
            canvas.translate((getWidth() - size) / 2, (getHeight() - size) / 2);
            canvas.scale(size / 24, size / 24);
            paint.setColor(color); paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(1.7f);
            paint.setStrokeCap(Paint.Cap.ROUND); paint.setStrokeJoin(Paint.Join.ROUND);
            switch (kind) {
                case "pen": path(canvas, 4, 20, 6, 13, 16, 3, 21, 8, 11, 18, 4, 20); path(canvas, 14, 5, 19, 10); break;
                case "eraser": path(canvas, 3, 14, 13, 4, 21, 12, 13, 20, 9, 20, 3, 14); path(canvas, 7, 10, 15, 18); path(canvas, 13, 20, 22, 20); break;
                case "move": path(canvas, 12, 2, 12, 22); path(canvas, 2, 12, 22, 12); path(canvas, 8, 6, 12, 2, 16, 6); path(canvas, 8, 18, 12, 22, 16, 18); path(canvas, 6, 8, 2, 12, 6, 16); path(canvas, 18, 8, 22, 12, 18, 16); break;
                case "history": canvas.drawArc(new RectF(4, 4, 21, 21), 210, 290, false, paint); path(canvas, 3, 3, 3, 9, 9, 9); path(canvas, 12, 7, 12, 13, 16, 15); break;
                case "document": case "page": path(canvas, 5, 2, 15, 2, 20, 7, 20, 22, 5, 22, 5, 2); path(canvas, 15, 2, 15, 7, 20, 7); path(canvas, 9, 12, 16, 12); path(canvas, 9, 16, 16, 16); break;
                case "eye": case "hidden":
                    Path eye = new Path(); eye.moveTo(2, 12); eye.quadTo(12, 0, 22, 12); eye.quadTo(12, 24, 2, 12); canvas.drawPath(eye, paint); canvas.drawCircle(12, 12, 3, paint);
                    if (kind.equals("hidden")) path(canvas, 3, 3, 21, 21); break;
                case "lock": canvas.drawRoundRect(5, 10, 19, 21, 2, 2, paint); canvas.drawArc(new RectF(8, 2, 16, 17), 180, 180, false, paint); path(canvas, 12, 14, 12, 17); break;
                case "collapse": path(canvas, 8, 5, 15, 12, 8, 19); path(canvas, 20, 5, 20, 19); break;
                case "expand": path(canvas, 16, 5, 9, 12, 16, 19); path(canvas, 4, 5, 4, 19); break;
                case "plus": path(canvas, 12, 4, 12, 20); path(canvas, 4, 12, 20, 12); break;
                case "more": paint.setStyle(Paint.Style.FILL); for (int x = 5; x <= 19; x += 7) canvas.drawCircle(x, 12, 1.7f, paint); break;
            }
            canvas.restore();
        }
        void path(Canvas canvas, float... coordinates) {
            Path path = new Path(); path.moveTo(coordinates[0], coordinates[1]);
            for (int i = 2; i < coordinates.length; i += 2) path.lineTo(coordinates[i], coordinates[i + 1]);
            canvas.drawPath(path, paint);
        }
    }
}
