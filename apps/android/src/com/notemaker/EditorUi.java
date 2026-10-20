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

/** Monochrome editor chrome. All tool panels overlay the unchanged canvas. */
final class EditorUi {
    private static final int INK = Color.BLACK;
    private static final int MUTED = 0xff555555;
    private static final int LINE = Color.BLACK;
    private static final int PAPER = Color.WHITE;
    private final MainActivity app;
    private final Map<String, Tool> tools = new LinkedHashMap<>();
    private final ArrayList<LayerPreview> previews = new ArrayList<>();
    private LinearLayout root, layerList;
    private TextView title, subtitle, connection;
    private FrameLayout workspace;
    private PopupWindow popup;
    private Tool popupAnchor;
    private Icon undoButton, redoButton;
    private StrokeSample sample;
    private TextView materialLabel;
    private View materialPanel;
    private boolean showToolLabels=true;
    private String selectedTool = "pen";
    private String layerState = "";
    private long thumbnailTime;
    private int thumbnailRevision = -1;

    EditorUi(MainActivity app) {
        this.app = app;
        showToolLabels=app.getSharedPreferences("settings",0).getBoolean("showToolLabels",true);
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
        header.setPadding(0, 0, dp(8), 0);
        topTool(header, "document", "Notes", "Document actions");
        LinearLayout heading = column();
        heading.setPadding(dp(12), 0, dp(8), 0);
        title = text("", 18, INK, false);
        title.setSingleLine(true);
        title.setEllipsize(TextUtils.TruncateAt.END);
        subtitle = text("", 11, MUTED, false);
        heading.addView(title);
        heading.addView(subtitle);
        header.addView(heading, new LinearLayout.LayoutParams(0, -2, 1));
        connection = text("", 10, MUTED, false);
        connection.setSingleLine(true);
        connection.setEllipsize(TextUtils.TruncateAt.END);
        connection.setOnClickListener(v->{if(!app.conflicts.isEmpty())showConflictPanel();});
        heading.addView(connection);
        topTool(header, "pen", "Pen", "Pen tools");
        topTool(header, "eraser", "Erase", "Eraser tools");
        View topRule=divider(); LinearLayout.LayoutParams ruleParams=new LinearLayout.LayoutParams(dp(1),dp(32));ruleParams.setMargins(dp(6),0,dp(6),0);header.addView(topRule,ruleParams);
        undoButton=icon("undo","Undo",()->{dismissPopup();app.history(false);});
        redoButton=icon("redo","Redo",()->{dismissPopup();app.history(true);});
        header.addView(undoButton,new LinearLayout.LayoutParams(dp(48),dp(56)));
        header.addView(redoButton,new LinearLayout.LayoutParams(dp(48),dp(56)));
        root.addView(header, new LinearLayout.LayoutParams(-1, dp(72)));
        root.addView(divider(), new LinearLayout.LayoutParams(-1, dp(1)));

        LinearLayout body = row();
        root.addView(body, new LinearLayout.LayoutParams(-1, 0, 1));
        LinearLayout rail = column();
        rail.setGravity(Gravity.CENTER_HORIZONTAL);
        rail.setPadding(0, dp(8), 0, dp(8));
        rail.setBackgroundColor(PAPER);
        ScrollView railScroll=new ScrollView(app);railScroll.setFillViewport(true);railScroll.setVerticalScrollBarEnabled(false);railScroll.addView(rail);
        body.addView(railScroll, new LinearLayout.LayoutParams(dp(56), -1));
        tool(rail, "template", "Template", "Page templates");
        tool(rail, "layers", "Layers", "Layer actions");
        tool(rail, "shapes", "Shapes", "Shape tools");
        tool(rail, "fill", "Fill", "Enclosed area fill");
        railRule(rail);
        tool(rail, "select", "Select", "Select and transform objects");
        tool(rail, "text", "Text", "Text placement");
        tool(rail, "insert", "Insert", "Insert content");
        tool(rail, "move", "Move", "Canvas navigation");
        railRule(rail);
        tool(rail, "page", "Pages", "Page actions");
        rail.addView(new View(app), new LinearLayout.LayoutParams(1, 0, 1));
        tool(rail, "agent", "Agent", "Open Agent");
        tool(rail, "more", "More", "More tools");
        body.addView(divider(), new LinearLayout.LayoutParams(dp(1), -1));

        workspace = new FrameLayout(app);
        body.addView(workspace, new LinearLayout.LayoutParams(0, -1, 1));
        app.canvas = app.new CanvasView(app);
        app.canvas.setContentDescription("Drawing canvas");
        workspace.addView(app.canvas, new FrameLayout.LayoutParams(-1, -1));
        return root;
    }

    private void topTool(LinearLayout header,String kind,String label,String description){
        Tool tool=new Tool(kind,label,description);tool.top=true;tools.put(kind,tool);
        header.addView(tool,new LinearLayout.LayoutParams(dp(54),-1));
        tool.setOnClickListener(v->openTool(kind,tool));
    }
    private void railRule(LinearLayout rail){
        LinearLayout.LayoutParams params=new LinearLayout.LayoutParams(dp(24),dp(1));params.setMargins(0,dp(7),0,dp(7));rail.addView(divider(),params);
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
        TextView button = text(label, 15, INK, false);
        button.setGravity(Gravity.CENTER_VERTICAL);
        button.setPadding(dp(14), dp(12), dp(14), dp(12));
        button.setMinHeight(dp(48));
        button.setBackgroundColor(PAPER);
        button.setContentDescription(label);
        button.setOnClickListener(v -> callback.run());
        return button;
    }
    private void menuAction(LinearLayout parent, String label, boolean enabled, Runnable callback) {
        parent.addView(divider(),new LinearLayout.LayoutParams(-1,dp(1)));
        LinearLayout button=row();button.setGravity(Gravity.CENTER_VERTICAL);button.setMinimumHeight(dp(52));
        button.setContentDescription(label);button.setEnabled(enabled);button.setAlpha(enabled?1f:.4f);
        String iconKind=actionIcon(label);Icon glyph=new Icon(iconKind);
        if(iconKind.equals("pen")||iconKind.equals("fountain")||iconKind.equals("pencil")||iconKind.equals("marker")||iconKind.equals("brush")||iconKind.equals("oily")||iconKind.equals("neoBrush")||iconKind.equals("charcoal")||iconKind.equals("latinCalligraphy")||iconKind.equals("asiaCalligraphy")||iconKind.equals("square"))glyph.accentColor=app.color;
        button.addView(glyph,new LinearLayout.LayoutParams(dp(38),dp(38)));
        TextView name=text(label,15,INK,false);name.setPadding(dp(10),dp(12),dp(4),dp(12));
        button.addView(name,new LinearLayout.LayoutParams(0,-2,1));
        button.setOnClickListener(v->{dismissPopup();callback.run();});
        parent.addView(button,new LinearLayout.LayoutParams(-1,-2));
    }
    private String actionIcon(String label){
        String s=label.toLowerCase(java.util.Locale.ROOT);
        if(s.equals("agent"))return "agent";
        if(s.contains("undo"))return "undo";if(s.contains("redo"))return "redo";
        if(s.contains("layer"))return "layers";if(s.contains("picture")||s.contains("artwork"))return "insert";
        if(s.contains("fountain"))return "fountain";
        if(s.contains("pencil"))return "pencil";
        if(s.contains("marker"))return "marker";
        if(s.contains("charcoal"))return "charcoal";
        if(s.contains("oily"))return "oily";
        if(s.contains("neobrush")||s.contains("neo brush"))return "neoBrush";
        if(s.contains("calligraphy"))return s.contains("asia")?"asiaCalligraphy":"latinCalligraphy";
        if(s.contains("square"))return "square";
        if(s.startsWith("pen")||s.contains(" pen")||s.contains("brush")||s.contains("preset"))return "pen";
        if(s.contains("text"))return "text";if(s.contains("erase"))return "eraser";
        if(s.contains("template")||s.equals("ruled")||s.equals("graph")||s.equals("dot grid"))return "template";
        if(s.contains("page")||s.contains("document"))return "document";
        if(s.contains("zoom")||s.contains("view"))return "move";
        if(s.contains("select")||s.contains("cop")||s.contains("paste")||s.contains("transform"))return "select";
        if(s.contains("done")||s.startsWith("ready"))return "check";return "more";
    }
    private void tool(LinearLayout rail, String icon, String label, String description) {
        Tool tool = new Tool(icon, label, description);
        tools.put(icon, tool);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(-1, dp(66));
        rail.addView(tool, params);
        tool.setOnClickListener(v -> openTool(icon, tool));
    }

    private void setText(TextView view, String value) {
        if (!value.contentEquals(view.getText())) view.setText(value);
    }

    void refresh() {
        setText(title, app.document+(app.model.deleted?" · Deleted":""));
        setText(subtitle, app.type.equals("pages") ? "NOTEBOOK  /  PAGE " + (app.page + 1)
                : app.type.equals("infinite") ? "INFINITE CANVAS" : "SINGLE PAGE");
        setText(connection, app.saveError!=null ? "Save failed" : app.saving>0 ? (app.replica.remote()?"Saving to proxy…":"Saving on device…") : app.connected
                ? app.pendingCount == 0 ? "●  Connected" : "●  Syncing changes"
                : app.replica.remote()?"○  Proxy unavailable · last snapshot":"○  Offline · saved on device");
        if(!app.conflicts.isEmpty())setText(connection,connection.getText()+"  ·  "+app.conflicts.size()+" conflict(s)");
        String mode = app.erase ? "eraser" : app.pan ? "move" : app.drawingShape!=null ? (app.drawingShape.equals("text")?"text":"shapes") : selectedTool;
        for (Map.Entry<String, Tool> entry : tools.entrySet()) {
            Tool tool=entry.getValue();
            tool.active(entry.getKey().equals(mode));
            boolean iconChanged=tool.icon.accentColor!=app.color;
            tool.icon.accentColor=app.color;
            if (entry.getKey().equals("pen")) {
                iconChanged|=!tool.icon.kind.equals(app.brush);
                tool.icon.kind=app.brush;
                String description="Pen tool, "+app.brush+" brush";
                if(!description.contentEquals(tool.getContentDescription()))tool.setContentDescription(description);
            }
            if(iconChanged)tool.icon.invalidate();
        }
        if (sample != null) sample.invalidate();
        if (materialLabel != null) setText(materialLabel,materialDescription());
        if (materialPanel != null) materialPanel.setVisibility("pen".equals(app.brush)?View.GONE:View.VISIBLE);
        boolean ready=!app.model.deleted&&!app.objectHistoryPending();
        undoButton.setEnabled(ready&&!app.undo.isEmpty());undoButton.setAlpha(undoButton.isEnabled()?1f:.3f);
        redoButton.setEnabled(ready&&!app.redo.isEmpty());redoButton.setAlpha(redoButton.isEnabled()?1f:.3f);

        StringBuilder state = new StringBuilder(app.document).append('/').append(app.page).append('/').append(app.selected);
        for (Map.Entry<String, JSONObject> entry : app.layers.entrySet()) {
            state.append('|').append(entry.getKey()).append(entry.getValue().toString()).append(app.hidden.contains(entry.getKey())).append(app.model.layerActive.get(entry.getKey()));
        }
        if (!state.toString().equals(layerState)) {
            layerState = state.toString();
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

    private LinearLayout layersPanel() {
        LinearLayout box=popupBody("Layers", "Topmost layer first");box.setTag(440);
        TextView add=action("＋  New layer",()->{dismissPopup();newLayer();});
        box.addView(add,new LinearLayout.LayoutParams(-1,dp(52)));
        box.addView(divider(),new LinearLayout.LayoutParams(-1,dp(1)));
        layerList=column();
        ScrollView scroll=new ScrollView(app);scroll.setFillViewport(false);scroll.addView(layerList);
        box.addView(scroll,new LinearLayout.LayoutParams(-1,dp(300)));
        rebuildLayers();
        return box;
    }

    private void rebuildLayers() {
        if(layerList==null)return;
        synchronized(app.lock){
            layerList.removeAllViews();previews.clear();
            ArrayList<String> ids=new ArrayList<>(app.layers.keySet());
            ids.removeIf(id->Boolean.FALSE.equals(app.model.layerActive.get(id)));Collections.reverse(ids);
            for(int index=0;index<ids.size();index++){
                String id=ids.get(index);JSONObject layer=app.layers.get(id);
                boolean selected=app.selected.equals(id),forced=layer.optBoolean("force"),visible=forced||!app.hidden.contains(id);
                LinearLayout card=row();card.setGravity(Gravity.CENTER_VERTICAL);card.setPadding(dp(8),dp(10),dp(8),dp(10));
                card.setBackground(surface(Color.WHITE,INK,0,selected?2:0));
                card.setContentDescription("Select layer "+layer.optString("name"));
                card.setOnClickListener(v->{synchronized(app.lock){app.selectLayer(id);}});
                card.setOnLongClickListener(v->{layerOptions(id,tools.get("layers"));return true;});
                Icon eye=icon(forced?"lock":visible?"eye":"hidden",
                    (forced?"Forced visible layer ":visible?"Hide layer ":"Show layer ")+layer.optString("name"),()->{
                        synchronized(app.lock){if(app.layers.get(id).optBoolean("force"))layerOptions(id,tools.get("layers"));
                        else{if(app.hidden.contains(id))app.hidden.remove(id);else app.hidden.add(id);app.save();app.update();}}
                    });
                card.addView(eye,new LinearLayout.LayoutParams(dp(48),dp(60)));
                LayerPreview preview=new LayerPreview(id,ids.size()-index,visible);previews.add(preview);
                card.addView(preview,new LinearLayout.LayoutParams(dp(48),dp(66)));
                LinearLayout details=column();details.setPadding(dp(14),0,dp(4),0);
                TextView name=text(layer.optString("name"),16,INK,selected);name.setMaxLines(2);name.setEllipsize(TextUtils.TruncateAt.END);details.addView(name);
                TextView state=text(forced?"For everyone":visible?"Visible":"Hidden",12,MUTED,false);state.setPadding(0,dp(6),0,0);details.addView(state);
                card.addView(details,new LinearLayout.LayoutParams(0,-2,1));
                card.addView(icon("more","Options for layer "+layer.optString("name"),()->layerOptions(id,tools.get("layers"))),new LinearLayout.LayoutParams(dp(48),dp(60)));
                layerList.addView(card,new LinearLayout.LayoutParams(-1,-2));
                layerList.addView(divider(),new LinearLayout.LayoutParams(-1,dp(1)));
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
                        app.deselect(); app.resetDrawingGesture(); app.selected = MainActivity.id();
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
            synchronized (app.lock) { app.hidden.remove(id); app.selectLayer(id); }
        });
        menuAction(box, "Rename layer…", true, () -> app.renameLayer(id));
        menuAction(box, "Archive layer", !id.equals("base"), () -> new AlertDialog.Builder(app).setTitle("Archive layer?").setMessage("This hides the layer and its contents. Undo restores it.").setNegativeButton("Cancel",null).setPositiveButton("Archive",(dialog,which)->app.layerCommand("hide",id,null)).show());
        menuAction(box, "Duplicate layer", true, () -> app.layerCommand("duplicate", id, null));
        ArrayList<String> order=new ArrayList<>(app.layers.keySet());int position=order.indexOf(id);
        // The list displays the highest order first.
        menuAction(box, "Move layer up", position>=0&&position+1<order.size(), () -> app.layerCommand("reorder", id, String.valueOf(position+1)));
        menuAction(box, "Move layer down", position>0, () -> app.layerCommand("reorder", id, String.valueOf(position-1)));
        menuAction(box, "Merge into another layer…", order.size()>1, () -> chooseMergeTarget(id,order));
        showPopup(box, anchor, false);
    }

    private void chooseMergeTarget(String source,ArrayList<String> ids){
        ArrayList<String> targets=new ArrayList<>();ArrayList<String> labels=new ArrayList<>();
        for(String id:ids)if(!id.equals(source)){targets.add(id);JSONObject layer=app.layers.get(id);labels.add(layer==null?id:layer.optString("name",id));}
        new AlertDialog.Builder(app).setTitle("Merge layer into").setItems(labels.toArray(new String[0]),(dialog,index)->app.layerCommand("merge",source,targets.get(index))).setNegativeButton("Cancel",null).show();
    }

    private LinearLayout popupBody(String heading, String description) {
        LinearLayout box = column();
        box.setPadding(dp(18), dp(16), dp(18), dp(12));
        box.setTag(320);
        box.setBackgroundColor(Color.WHITE);
        TextView label=text(heading,18,INK,false);label.setPadding(0,0,0,dp(12));box.addView(label);
        if (!description.isEmpty()) {
            TextView help = text(description, 12, MUTED, false);
            help.setPadding(0, dp(6), 0, dp(10));
            box.addView(help);
        }
        return box;
    }

    void openTool(String kind, View anchor) {
        if(!kind.equals("select")&&(app.selecting||app.selectionSession!=null||!app.selectedEntities.isEmpty()))app.deselect();
        app.fillTapMode=false;
        if (Arrays.asList("pen","eraser","move","shapes","text","fill","select").contains(kind)) selectedTool=kind;
        if(Arrays.asList("pen","eraser","move","shapes","text","fill","select").contains(kind))app.selecting=kind.equals("select");
        LinearLayout box;
        switch (kind) {
            case "agent": dismissPopup();app.agent.show();return;
            case "layers": box=layersPanel();break;
            case "template": box=templatePanel();break;
            case "insert":
                box=popupBody("Insert", "");
                menuAction(box,"Insert picture…",true,app::choosePicture);
                menuAction(box,"Insert SVG artwork…",true,app::chooseSvgPicture);
                menuAction(box,"Enter text…",true,app::textPlacementDialog);
                break;
            case "more":
                box=popupBody("More", "");
                menuAction(box,"Toolbar preferences…",true,()->showPopup(preferencesPanel(),tools.get("more"),false));
                menuAction(box,"Network settings…",true,()->showPopup(networkSettingsPanel(),tools.get("more"),false));
                menuAction(box,"Pen presets…",true,()->showPopup(presetPanel(),tools.get("pen"),false));
                menuAction(box,"Undo and redo",true,()->openTool("history",tools.get("more")));
                menuAction(box,"Agent",true,()->app.agent.show());
                if(!app.conflicts.isEmpty())menuAction(box,"Sync conflicts",true,this::showConflictPanel);
                break;
            case "pen":
                app.erase = false; app.pan = false; app.drawingShape=null;app.pendingText=null;if(app.canvas!=null)app.canvas.shapeDragging=false; app.update();
                box = penPanel();
                break;
            case "eraser":
                app.erase = true; app.pan = false; app.drawingShape=null;app.pendingText=null;if(app.canvas!=null)app.canvas.shapeDragging=false; app.update();
                box = eraserPanel();
                menuAction(box, "Undo last action", !app.undo.isEmpty(), () -> app.history(false));
                menuAction(box, "Ready to erase", true, () -> {});
                break;
            case "move":
                app.pan = true; app.erase = false; app.drawingShape=null;app.pendingText=null;if(app.canvas!=null)app.canvas.shapeDragging=false; app.update();
                box = popupBody("Move canvas", "Drag to move. Pinch to zoom.");
                menuAction(box, "Zoom out", true, () -> { app.canvas.zoom = Math.max(.2f, app.canvas.zoom / 1.25f); app.canvas.invalidate(); });
                menuAction(box, "Zoom in", true, () -> { app.canvas.zoom = Math.min(5f, app.canvas.zoom * 1.25f); app.canvas.invalidate(); });
                menuAction(box, "Reset view", true, () -> { app.canvas.zoom = 1; app.canvas.offsetX = app.canvas.offsetY = 0; app.canvas.invalidate(); });
                menuAction(box, "Return to pen", true, () -> { selectedTool="pen";app.pan = app.erase = false; app.update(); });
                break;
            case "shapes":
                app.pan=false;app.erase=false;app.pendingText=null;app.update();
                box=shapePanel();
                break;
            case "text":
                app.pan=false;app.erase=false;app.drawingShape=null;app.pendingText=null;if(app.canvas!=null)app.canvas.shapeDragging=false;app.update();
                box=popupBody("Text", "Enter text and font size, then tap the canvas to place it.");
                menuAction(box,"Enter text…",true,app::textPlacementDialog);
                for(Map<String,Object> entity:app.selectedEntities)if("object".equals(entity.get("kind"))){Map<String,Object> selected=app.model.objects.get(String.valueOf(entity.get("id")));if(selected!=null&&"text".equals(selected.get("shape"))){String objectId=String.valueOf(entity.get("id"));menuAction(box,"Edit selected text…",true,()->app.editObject(objectId));break;}}
                menuAction(box,"Edit placed text…",app.model.objects.values().stream().anyMatch(o -> "text".equals(o.get("shape")) && !Boolean.FALSE.equals(app.model.objectActive.get(o.get("object")))),app::objectDialog);
                break;
            case "fill":
                app.pan=false;app.erase=false;app.drawingShape=null;app.pendingText=null;app.fillTapMode=false;app.update();box=fillPanel();break;
            case "select":
                app.pan=false;app.erase=false;app.drawingShape=null;app.pendingText=null;app.selecting=true;app.update();box=selectionPanel();break;
            case "conflicts":
                box=conflictsPanel();break;
            case "history":
                box = popupBody("History", "Your drawing and erasing actions");
                boolean historyReady=!app.objectHistoryPending();
                menuAction(box, "Undo", historyReady&&!app.undo.isEmpty(), () -> app.history(false));
                menuAction(box, "Redo", historyReady&&!app.redo.isEmpty(), () -> app.history(true));
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
                menuAction(box, "Browse files…", true, () -> new DocumentBrowser(app).show());
                menuAction(box, "Open or create document…", true, app::documentDialog);
                menuAction(box, "Edit diagram objects…", !app.model.objects.isEmpty(), app::objectDialog);
                break;
        }
        showPopup(box, anchor, false);
    }

    private void changePage(int page) { app.deselect();synchronized (app.lock) { app.resetDrawingGesture();app.page = page; app.save(); app.update(); } }

    private LinearLayout shapePanel(){
        LinearLayout box=popupBody("Shapes", "Choose a shape, then drag on the canvas.");box.setTag(480);
        box.addView(new StrokeSample(),new LinearLayout.LayoutParams(-1,dp(64)));
        section(box,"Shapes");
        LinearLayout grid=column();
        ArrayList<String> ids=new ArrayList<>(com.notemaker.core.ShapeCatalog.ids());
        ids.removeAll(Arrays.asList("circle","rectangle","rhombus","arrowLine","waveLine","regularHexagon","text","svg","image"));
        // Six columns on tablets, four on narrow displays, always at least 48dp targets.
        int columns=app.getResources().getConfiguration().screenWidthDp<500?4:6;
        for(int i=0;i<ids.size();i++){
            if(i%columns==0)grid.addView(row(),new LinearLayout.LayoutParams(-1,dp(58)));
            LinearLayout row=(LinearLayout)grid.getChildAt(grid.getChildCount()-1);
            String id=ids.get(i),label=com.notemaker.core.ShapeCatalog.name(id);
            View choice=choice(id,"Create "+label,()->id.equals(app.drawingShape),()->{dismissPopup();selectedTool="shapes";app.setDrawingTool(id);},false);
            row.addView(choice,new LinearLayout.LayoutParams(0,-1,1));
        }
        LinearLayout last=(LinearLayout)grid.getChildAt(grid.getChildCount()-1);
        while(last.getChildCount()<columns)last.addView(new View(app),new LinearLayout.LayoutParams(0,-1,1));
        box.addView(grid);
        section(box,"Colors");box.addView(colorChoices(true));
        section(box,"Line width");box.addView(widthControl());
        CheckBox fill=new CheckBox(app);fill.setText("Shape fill");fill.setTextColor(INK);fill.setButtonTintList(ColorStateList.valueOf(INK));fill.setChecked(app.objectFillColor!=Color.TRANSPARENT);
        fill.setOnCheckedChangeListener((button,checked)->app.objectFillColor=checked?app.color:Color.TRANSPARENT);box.addView(fill);
        return box;
    }

    private void section(LinearLayout box,String title){
        LinearLayout.LayoutParams rule=new LinearLayout.LayoutParams(-1,dp(1));rule.setMargins(0,dp(12),0,dp(12));box.addView(divider(),rule);
        TextView label=text(title,15,INK,false);label.setPadding(0,0,0,dp(8));box.addView(label);
    }

    private View choice(String kind,String description,java.util.function.BooleanSupplier selected,Runnable callback,boolean label){
        LinearLayout item=new LinearLayout(app){
            final Paint marker=new Paint(Paint.ANTI_ALIAS_FLAG);
            @Override protected void dispatchDraw(Canvas canvas){super.dispatchDraw(canvas);if(selected.getAsBoolean()){marker.setColor(INK);canvas.drawCircle(getWidth()/2f,getHeight()-dp(5),dp(3),marker);}}
        };
        item.setOrientation(LinearLayout.VERTICAL);item.setGravity(Gravity.CENTER);item.setPadding(dp(2),dp(4),dp(2),dp(12));
        item.setContentDescription(description);item.setTooltipText(description);
        Icon choiceIcon=new Icon(kind);choiceIcon.accentColor=app.color;
        item.addView(choiceIcon,new LinearLayout.LayoutParams(dp(40),dp(36)));
        if(label){TextView name=text(description,11,INK,false);name.setGravity(Gravity.CENTER);name.setSingleLine(true);name.setEllipsize(TextUtils.TruncateAt.END);item.addView(name,new LinearLayout.LayoutParams(-1,-2));}
        item.setOnClickListener(v->{callback.run();ViewParent parent=item.getParent();if(parent instanceof ViewGroup){ViewGroup group=(ViewGroup)parent;for(int i=0;i<group.getChildCount();i++)group.getChildAt(i).invalidate();}});
        return item;
    }

    private LinearLayout colorChoices(boolean shape){
        LinearLayout grid=column();
        int[] colors={Color.BLACK,0xff555555,0xffaaaaaa,Color.WHITE,0xffbd2424,0xff245dbd,0xff247d38,0xff7824aa,0xffd06a00,0xffffe59a};
        String[] names={"Black","Dark gray","Gray","White","Red","Blue","Green","Purple","Orange","Yellow"};
        for(int i=0;i<colors.length;i++){
            if(i%5==0)grid.addView(row(),new LinearLayout.LayoutParams(-1,dp(48)));
            final int color=colors[i];View swatch=new View(app){
                final Paint p=new Paint(Paint.ANTI_ALIAS_FLAG);
                protected void onDraw(Canvas c){float x=getWidth()/2f,y=dp(19);p.setStyle(Paint.Style.FILL);p.setColor(color);c.drawCircle(x,y,dp(13),p);p.setStyle(Paint.Style.STROKE);p.setColor(INK);p.setStrokeWidth(dp(1));c.drawCircle(x,y,dp(13),p);if(app.color==color){p.setStyle(Paint.Style.FILL);c.drawCircle(x,dp(40),dp(3),p);}}
            };
            swatch.setContentDescription(names[i]+" ink");swatch.setTooltipText(names[i]+" ink");
            swatch.setOnClickListener(v->{app.color=color;if(sample!=null)sample.invalidate();if(selectedTool.equals("fill"))app.objectFillColor=color;if(shape&&app.objectFillColor!=Color.TRANSPARENT)app.objectFillColor=color;app.update();for(int r=0;r<grid.getChildCount();r++){ViewGroup row=(ViewGroup)grid.getChildAt(r);for(int c=0;c<row.getChildCount();c++)row.getChildAt(c).invalidate();}});
            ((LinearLayout)grid.getChildAt(grid.getChildCount()-1)).addView(swatch,new LinearLayout.LayoutParams(0,-1,1));
        }
        return grid;
    }

    private View widthControl(){
        LinearLayout row=row();row.setGravity(Gravity.CENTER_VERTICAL);
        TextView value=text(String.valueOf((int)app.width),15,INK,false);value.setGravity(Gravity.RIGHT);
        SeekBar width=new SeekBar(app);width.setContentDescription("Pen width");width.setMax(29);width.setProgress(Math.max(0,(int)app.width-1));
        width.setProgressTintList(ColorStateList.valueOf(INK));width.setProgressBackgroundTintList(ColorStateList.valueOf(0xffbbbbbb));width.setThumbTintList(ColorStateList.valueOf(INK));
        width.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onProgressChanged(SeekBar bar,int v,boolean user){if(user){app.width=v+1;value.setText(String.valueOf(v+1));if(sample!=null)sample.invalidate();}}public void onStartTrackingTouch(SeekBar b){}public void onStopTrackingTouch(SeekBar b){}});
        row.addView(width,new LinearLayout.LayoutParams(0,dp(48),1));row.addView(value,new LinearLayout.LayoutParams(dp(40),-2));return row;
    }

    private final class StrokeSample extends View{
        final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        StrokeSample(){super(app);sample=this;setContentDescription("Stroke preview");}
        protected void onDraw(Canvas c){
            float w=getWidth(),h=getHeight(),left=dp(14),right=w-dp(14),mid=h*.52f,amplitude=Math.min(dp(13),h*.27f);
            ArrayList<Object> points=new ArrayList<>();
            for(int i=0;i<=160;i++){
                double t=i/160.0,x=left+(right-left)*t;
                double y=mid+Math.sin(t*Math.PI*3)*amplitude*(.82+.18*Math.cos(t*Math.PI*2));
                double pressure=.35+.55*(.5+.5*Math.sin(t*Math.PI*2-.7));
                points.add(Arrays.asList(x,y,pressure));
            }
            Map<String,Object> stroke=new LinkedHashMap<>();
            stroke.put("kind","stroke");stroke.put("brush",app.brush);stroke.put("color",app.color);stroke.put("material",app.material);
            stroke.put("width",dp(Math.max(1,Math.min(8,app.width))));stroke.put("points",points);
            c.save();c.clipRect(left-dp(2),dp(3),right+dp(2),h-dp(3));
            com.notemaker.core.Render.drawStroke(stroke,new com.notemaker.cli.Png(c));
            c.restore();
        }
    }

    private String materialDescription(){
        String range;
        switch(app.brush){
            case "pencil":range="Graphite grade · HB to 6B";break;
            case "marker":range="Opacity · light to saturated";break;
            case "brush":range="Bristles · sparse to dense";break;
            case "oily":range="Paint load · lean to loaded";break;
            case "neoBrush":range="Softness · focused to feathered";break;
            case "charcoal":range="Charcoal · compressed to dusty";break;
            case "fountain":range="Nib flex · fine to pressure-responsive";break;
            case "latinCalligraphy":range="Nib angle · 25° to 65°";break;
            case "asiaCalligraphy":range="Nib angle · 40° to 80°";break;
            case "square":range="Chisel width · narrow to broad";break;
            default:range="Material response";break;
        }
        return range+" · "+app.material+"%";
    }
    private View materialControl(){
        LinearLayout group=column();materialLabel=text(materialDescription(),13,INK,false);group.addView(materialLabel);
        SeekBar seek=new SeekBar(app);seek.setContentDescription("Brush material");seek.setMax(100);seek.setProgress(app.material);
        seek.setProgressTintList(ColorStateList.valueOf(INK));seek.setProgressBackgroundTintList(ColorStateList.valueOf(0xffbbbbbb));seek.setThumbTintList(ColorStateList.valueOf(INK));
        seek.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onProgressChanged(SeekBar bar,int value,boolean user){if(user){app.material=value;materialLabel.setText(materialDescription());app.getSharedPreferences("settings",0).edit().putInt("penMaterial",value).apply();if(sample!=null)sample.invalidate();}}public void onStartTrackingTouch(SeekBar bar){}public void onStopTrackingTouch(SeekBar bar){}});
        group.addView(seek,new LinearLayout.LayoutParams(-1,dp(42)));return group;
    }

    private LinearLayout penPanel(){
        LinearLayout box=popupBody("Pen", "");box.setTag(480);
        LinearLayout brushes=row();
        for(String brush:new String[]{"pen","fountain","pencil","brush","marker"}){
            String name=Character.toUpperCase(brush.charAt(0))+brush.substring(1);
            brushes.addView(choice(brush,name,()->app.brush.equals(brush),()->{app.brush=brush;app.update();},true),new LinearLayout.LayoutParams(0,dp(70),1));
        }
        box.addView(brushes);box.addView(new StrokeSample(),new LinearLayout.LayoutParams(-1,dp(64)));
        section(box,"Line width");box.addView(widthControl());
        materialPanel=materialControl();box.addView(materialPanel);
        section(box,"Colors");box.addView(colorChoices(false));
        menuAction(box,"More brush types…",true,()->showPopup(brushPanel(),tools.get("pen"),false));
        menuAction(box,"Presets…",true,()->showPopup(presetPanel(),tools.get("pen"),false));
        menuAction(box,"Done",true,()->{});return box;
    }

    private LinearLayout presetPanel() {
        LinearLayout box = popupBody("Pen presets", "Saved presets keep color and width together.");
        android.content.SharedPreferences prefs = app.getSharedPreferences("settings", 0);
        String[] defaults = {"Black · 2", "Black · 5", "Black · 9", "Red · 5", "Blue · 5"};
        int[] colors = {Color.BLACK, Color.BLACK, Color.BLACK, 0xffbd2424, 0xff245dbd};
        int[] widths = {2, 5, 9, 5, 5};
        for (int i = 0; i < defaults.length; i++) {
            final int color = colors[i], width = widths[i];
            menuAction(box, defaults[i], true, () -> applyPenPreset(color, width, "pen", 50));
        }
        String saved = prefs.getString("penPresets", "");
        if (!saved.isEmpty()) {
            try {
                org.json.JSONArray list = new org.json.JSONArray(saved);
                for (int i = 0; i < list.length(); i++) {
                    org.json.JSONObject preset = list.optJSONObject(i);
                    if (preset == null) continue;
                    final int color = preset.optInt("color", Color.BLACK), width = preset.optInt("width", 5), material=preset.optInt("material",50);final String brush=preset.optString("brush","pen");
                    menuAction(box, preset.optString("name", "Custom") + " · " + width, true, () -> applyPenPreset(color, width, brush, material));
                }
            } catch (Exception ignored) { }
        }
        menuAction(box, "Save current as preset…", true, () -> savePenPreset());
        return box;
    }

    private LinearLayout brushPanel() {
        LinearLayout box=popupBody("Brush types","Choose a monochrome brush profile.");
        for(String brush:new String[]{"oily","fountain","charcoal","neoBrush","latinCalligraphy","asiaCalligraphy","square"}){
            String label=brush.replaceAll("([a-z])([A-Z])","$1 $2");
            menuAction(box,label,true,()->{app.brush=brush;app.update();dismissPopup();});
        }
        return box;
    }

    private LinearLayout preferencesPanel(){
        LinearLayout box=popupBody("Toolbar preferences","Keep controls readable on the monochrome display.");
        CheckBox labels=new CheckBox(app);labels.setText("Show tool labels");labels.setTextColor(INK);labels.setButtonTintList(ColorStateList.valueOf(INK));labels.setChecked(showToolLabels);box.addView(labels);
        labels.setOnCheckedChangeListener((button,checked)->{showToolLabels=checked;app.getSharedPreferences("settings",0).edit().putBoolean("showToolLabels",checked).apply();for(Tool item:tools.values())item.label.setVisibility(checked?View.VISIBLE:View.GONE);});
        menuAction(box,"Save current pen as preset…",true,()->savePenPreset());
        return box;
    }

    private LinearLayout networkSettingsPanel(){
        LinearLayout box=popupBody("Network settings","Choose where notes sync and where the editing service runs.");
        box.setTag(480);
        EditText server=new EditText(app);server.setSingleLine();server.setText(MainActivity.serverAddressForSettings(app.host));server.setHint("192.168.50.180");
        server.setInputType(android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_VARIATION_URI);
        box.addView(text("Notes server address",12,INK,true));box.addView(server);
        EditText serverPort=new EditText(app);serverPort.setSingleLine();serverPort.setText(MainActivity.serverPortForSettings(app.host));serverPort.setHint("8765");serverPort.setInputType(android.text.InputType.TYPE_CLASS_NUMBER);
        box.addView(text("Server port",12,INK,true));box.addView(serverPort);
        TextView serverHelp=text("Leave the address blank for local-only notes. A blank port uses 8765. For the Android emulator, use 10.0.2.2. On your LAN, enter the computer running NoteMaker, for example 192.168.50.180.",11,MUTED,false);serverHelp.setPadding(0,dp(4),0,dp(8));box.addView(serverHelp);
        EditText proxy=new EditText(app);proxy.setSingleLine();proxy.setText(app.proxyUrl);proxy.setHint("Optional editing proxy URL");proxy.setInputType(android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_VARIATION_URI);
        box.addView(text("Editing proxy URL",12,INK,true));box.addView(proxy);
        EditText token=new EditText(app);token.setSingleLine();token.setText(app.proxyToken);token.setHint("Proxy bearer token");token.setInputType( android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD);
        box.addView(text("Editing proxy token",12,INK,true));box.addView(token);
        TextView proxyHelp=text("Optional. Leave both proxy fields blank to use this device’s local editing service. A proxy URL requires a token.",11,MUTED,false);proxyHelp.setPadding(0,dp(4),0,dp(8));box.addView(proxyHelp);
        menuAction(box,"Save network settings",true,()->app.saveNetworkSettings(server.getText().toString(),serverPort.getText().toString(),proxy.getText().toString(),token.getText().toString()));
        menuAction(box,"Done",true,()->dismissPopup());
        return box;
    }

    private LinearLayout fillPanel() {
        LinearLayout box = popupBody("Enclosed fill", "Choose a fill color, then fill a closed object or ink path.");
        box.setTag(400);if(app.objectFillColor==Color.TRANSPARENT)app.objectFillColor=app.color;
        box.addView(colorChoices(true));
        menuAction(box,"Tap enclosed area to fill",true,()->{app.fillTapMode=true;app.selecting=false;app.objectFillColor=app.objectFillColor==Color.TRANSPARENT?app.color:app.objectFillColor;dismissPopup();app.update();Toast.makeText(app,"Tap inside a closed area",Toast.LENGTH_SHORT).show();});
        menuAction(box, "Apply to a shape…", app.objectFillColor != Color.TRANSPARENT && !app.model.objects.isEmpty(), () -> {
            dismissPopup();
            LinearLayout targets = popupBody("Fill a shape", "Choose a native object.");
            for (Map.Entry<String, Map<String,Object>> entry : app.model.objects.entrySet()) {
                if (Boolean.FALSE.equals(app.model.objectActive.get(entry.getKey()))) continue;
                String id=entry.getKey();Map<String,Object> object=entry.getValue();
                menuAction(targets, com.notemaker.core.ShapeCatalog.name(String.valueOf(object.get("shape"))) + " · " + id.substring(0,Math.min(8,id.length())), true, () -> app.fillEntity("object",id,app.objectFillColor));
            }
            showPopup(targets, tools.get("fill"), false);
        });
        menuAction(box, "Apply to a closed ink path…", app.objectFillColor != Color.TRANSPARENT && !app.strokes.isEmpty(), () -> {
            dismissPopup();
            LinearLayout targets = popupBody("Fill a path", "Open paths are rejected by the shared validator.");
            for (Map.Entry<String, org.json.JSONObject> entry : app.strokes.entrySet()) {
                if (Boolean.FALSE.equals(app.active.get(entry.getKey())) || !app.selected.equals(entry.getValue().optString("layer")) || entry.getValue().optInt("page") != app.page) continue;
                String id=entry.getKey();
                menuAction(targets, "Ink path · " + id.substring(0,Math.min(8,id.length())), true, () -> app.fillEntity("stroke",id,app.objectFillColor));
            }
            showPopup(targets, tools.get("fill"), false);
        });
        return box;
    }

    private LinearLayout eraserPanel() {
        LinearLayout box=popupBody("Eraser", "Erase items on the selected layer. Each gesture saves as one action.");
        box.setTag(400);
        LinearLayout modes=row();
        for(String mode:new String[]{"stroke","partial","area"}){
            String label=mode.equals("stroke")?"Whole item":Character.toUpperCase(mode.charAt(0))+mode.substring(1)+" erase";
            modes.addView(choice("eraser",label,()->app.eraserMode.equals(mode),()->{app.eraserMode=mode;app.update();},true),new LinearLayout.LayoutParams(0,dp(72),1));
        }
        box.addView(modes);section(box,"Options");
        CheckBox scribble=new CheckBox(app);scribble.setText("Scribble to erase");scribble.setChecked(app.scribbleToErase);scribble.setTextColor(INK);scribble.setButtonTintList(ColorStateList.valueOf(INK));scribble.setOnCheckedChangeListener((button,checked)->{app.scribbleToErase=checked;app.getSharedPreferences("settings",0).edit().putBoolean("scribbleToErase",checked).apply();});box.addView(scribble);
        TextView radiusLabel=text("Eraser size  "+(int)app.eraserRadius,12,INK,true);radiusLabel.setPadding(0,dp(6),0,0);box.addView(radiusLabel);
        SeekBar radius=new SeekBar(app);radius.setMax(79);radius.setProgress(Math.max(0,(int)app.eraserRadius-1));radius.setProgressTintList(ColorStateList.valueOf(INK));radius.setThumbTintList(ColorStateList.valueOf(INK));radius.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onProgressChanged(SeekBar b,int v,boolean user){app.eraserRadius=v+1;radiusLabel.setText("Eraser size  "+(v+1));}public void onStartTrackingTouch(SeekBar b){}public void onStopTrackingTouch(SeekBar b){}});box.addView(radius,new LinearLayout.LayoutParams(-1,dp(42)));
        menuAction(box,"Erase page",true,()->app.applyErase("page",app.selected,app.page,new org.json.JSONArray(),app.eraserRadius));
        menuAction(box,"Erase objects…",!app.model.objects.isEmpty(),app::objectDialog);
        return box;
    }

    private LinearLayout selectionPanel(){
        LinearLayout box=popupBody("Selection","Selected objects are locked while you transform them.");
        TextView state=text(app.selectedEntities.size()+" selected · "+app.selectionSkipped+" skipped as locked",12,MUTED,false);state.setPadding(0,dp(4),0,dp(8));box.addView(state);
        CheckBox lasso=new CheckBox(app);lasso.setText("Freeform lasso (off: rectangle)");lasso.setTextColor(INK);lasso.setButtonTintList(ColorStateList.valueOf(INK));lasso.setChecked(app.freeformLasso);box.addView(lasso);lasso.setOnCheckedChangeListener((button,checked)->{app.freeformLasso=checked;app.getSharedPreferences("settings",0).edit().putBoolean("freeformLasso",checked).apply();app.update();});
        menuAction(box,"Choose objects and strokes…",true,()->chooseEntities());
        menuAction(box,"Select all unlocked on this page",true,app::selectAllVisible);
        menuAction(box,"Tap an item or draw a selection boundary",true,()->{});
        menuAction(box,"Preview scale up",!app.selectedEntities.isEmpty(),()->app.previewSelection(1.1,0,0));
        menuAction(box,"Preview scale down",!app.selectedEntities.isEmpty(),()->app.previewSelection(.9,0,0));
        menuAction(box,"Preview rotate clockwise",!app.selectedEntities.isEmpty(),()->app.previewTransform(com.notemaker.core.Json.object("scale",1,"dx",0,"dy",0,"rotation",15)));
        menuAction(box,"Preview flip horizontal",!app.selectedEntities.isEmpty(),()->app.previewTransform(com.notemaker.core.Json.object("scale",1,"dx",0,"dy",0,"flipX",true)));
        menuAction(box,"Preview flip vertical",!app.selectedEntities.isEmpty(),()->app.previewTransform(com.notemaker.core.Json.object("scale",1,"dx",0,"dy",0,"flipY",true)));
        menuAction(box,"Preview move right",!app.selectedEntities.isEmpty(),()->app.previewSelection(1,24,0));
        menuAction(box,"Preview move down",!app.selectedEntities.isEmpty(),()->app.previewSelection(1,0,24));
        menuAction(box,"Commit transform",app.pendingTransform!=null,app::commitTransformPreview);
        menuAction(box,"Cancel preview",app.pendingTransform!=null,app::cancelTransformPreview);
        menuAction(box,"Duplicate selection",!app.selectedEntities.isEmpty(),app::duplicateSelection);
        menuAction(box,"Delete selection",!app.selectedEntities.isEmpty(),app::deleteSelection);
        menuAction(box,"Cut selection",!app.selectedEntities.isEmpty(),app::cutSelection);
        menuAction(box,"Copy selection",!app.selectedEntities.isEmpty(),app::copySelection);
        menuAction(box,"Paste copied selection",!com.notemaker.core.Json.list(app.selectionClipboard.getOrDefault("entities",java.util.Collections.emptyList())).isEmpty(),app::pasteSelection);
        menuAction(box,"Deselect and release locks",app.selectionSession!=null,app::deselect);
        return box;
    }

    private LinearLayout conflictsPanel(){
        LinearLayout box=popupBody("Sync conflicts","Both offline versions are preserved until you choose.");
        for(Object raw:app.conflicts){Map<String,Object> conflict=com.notemaker.core.Json.map(raw);String id=String.valueOf(conflict.get("id"));String title=conflict.getOrDefault("kind","Edit")+" · "+conflict.getOrDefault("entity",id);
            menuAction(box,String.valueOf(title),true,()->showConflict(id,conflict));
        }
        return box;
    }

    private void showConflict(String id,Map<String,Object> conflict){
        dismissPopup();LinearLayout box=popupBody("Compare versions","Choose either version or keep both. Actor details appear when recorded.");
        LinearLayout columns=new LinearLayout(app);columns.setOrientation(LinearLayout.HORIZONTAL);
        String actorA=String.valueOf(conflict.getOrDefault("localActor","actor not recorded")),actorB=String.valueOf(conflict.getOrDefault("remoteActor","actor not recorded"));
        columns.addView(conflictVersion("Version A",actorA,conflict.get("local")),new LinearLayout.LayoutParams(0,dp(190),1));
        columns.addView(conflictVersion("Version B",actorB,conflict.get("remote")),new LinearLayout.LayoutParams(0,dp(190),1));box.addView(columns);
        TextView details=text("Version A details\n"+versionSummary(conflict.get("local"))+"\nVersion B details\n"+versionSummary(conflict.get("remote")),11,MUTED,false);details.setTextIsSelectable(true);box.addView(details);
        menuAction(box,"Use Version A",true,()->app.resolveConflict(id,"local"));
        menuAction(box,"Use Version B",true,()->app.resolveConflict(id,"remote"));
        menuAction(box,"Keep both versions",true,()->app.resolveConflict(id,"both"));
        showPopup(box,connection,false);
    }

    private LinearLayout conflictVersion(String title,String actor,Object raw){
        LinearLayout column=new LinearLayout(app);column.setOrientation(LinearLayout.VERTICAL);column.setPadding(dp(4),dp(4),dp(4),dp(4));
        TextView heading=text(title+" · "+actor,12,INK,true);column.addView(heading);ConflictPreview preview=new ConflictPreview(raw);column.addView(preview,new LinearLayout.LayoutParams(-1,dp(120)));
        if(raw instanceof Map){Map<String,Object> value=com.notemaker.core.Json.map(raw);TextView label=text(String.valueOf(value.getOrDefault("shape",value.get("kind")))+(value.containsKey("text")?" · "+value.get("text"):""),11,MUTED,false);column.addView(label);}
        else column.addView(text("Deleted or unavailable",11,MUTED,false));
        return column;
    }

    private final class ConflictPreview extends View{
        final Object raw;final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        ConflictPreview(Object raw){super(app);this.raw=raw;setBackgroundColor(0xffffffff);setContentDescription("Conflict version preview");}
        @Override protected void onDraw(Canvas canvas){super.onDraw(canvas);if(!(raw instanceof Map)){canvas.drawText("No version",dp(8),dp(24),paint);return;}Map<String,Object> value=com.notemaker.core.Json.map(raw);try{
            double left=0,top=0,right=100,bottom=100;
            if(value.containsKey("points")){List<Object> points=com.notemaker.core.Json.list(value.get("points"));if(points.isEmpty())return;left=top=Double.POSITIVE_INFINITY;right=bottom=Double.NEGATIVE_INFINITY;for(Object item:points){List<Object> point=com.notemaker.core.Json.list(item);left=Math.min(left,((Number)point.get(0)).doubleValue());right=Math.max(right,((Number)point.get(0)).doubleValue());top=Math.min(top,((Number)point.get(1)).doubleValue());bottom=Math.max(bottom,((Number)point.get(1)).doubleValue());}}
            else if(value.containsKey("x")){left=((Number)value.get("x")).doubleValue();top=((Number)value.get("y")).doubleValue();right=left+((Number)value.get("w")).doubleValue();bottom=top+((Number)value.get("h")).doubleValue();}
            double width=Math.max(24,right-left),height=Math.max(24,bottom-top),scale=Math.min((getWidth()-dp(16))/width,(getHeight()-dp(16))/height);scale=Math.max(.01,Math.min(4,scale));canvas.save();canvas.translate((float)((getWidth()-width*scale)/2-left*scale),(float)((getHeight()-height*scale)/2-top*scale));canvas.scale((float)scale,(float)scale);
            if(value.containsKey("points")){int color=((Number)value.getOrDefault("color",Color.BLACK)).intValue();paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)((Number)value.getOrDefault("width",4)).doubleValue());paint.setStrokeCap(Paint.Cap.ROUND);paint.setStrokeJoin(Paint.Join.ROUND);List<Object> points=com.notemaker.core.Json.list(value.get("points"));Path path=new Path();for(int i=0;i<points.size();i++){List<Object> point=com.notemaker.core.Json.list(points.get(i));if(i==0)path.moveTo(((Number)point.get(0)).floatValue(),((Number)point.get(1)).floatValue());else path.lineTo(((Number)point.get(0)).floatValue(),((Number)point.get(1)).floatValue());}canvas.drawPath(path,paint);paint.setStyle(Paint.Style.FILL);}
            else com.notemaker.core.Render.drawObject(app.model,value,new com.notemaker.cli.Png(canvas));canvas.restore();
        }catch(Exception ignored){}}
    }

    private String pretty(Object value){try{return value==null?"(empty)":new org.json.JSONObject(com.notemaker.core.Json.write(value)).toString(2);}catch(Exception error){return String.valueOf(value);}}

    private String versionSummary(Object raw){
        if(!(raw instanceof Map))return pretty(raw);Map<String,Object> version=com.notemaker.core.Json.map(raw);StringBuilder out=new StringBuilder();
        for(String key:new String[]{"kind","shape","text","fontFamily","alignment","bold","italic","underline","x","y","w","h","fontSize","color","fill","brush","width","active"})if(version.containsKey(key))out.append(key).append(": ").append(version.get(key)).append('\n');
        Object points=version.get("points");if(points instanceof java.util.List)out.append("points: ").append(((java.util.List<?>)points).size()).append('\n');
        return out.toString().trim();
    }

    private void chooseEntities(){
        ArrayList<Map<String,Object>> entities=new ArrayList<>();ArrayList<String> labels=new ArrayList<>();
        synchronized(app.lock){
            if(app.canvas!=null&&app.canvas.visible(app.selected))for(com.notemaker.core.DrawItem item:app.layerEditor().items()){
                String owner=leaseActor(item.kind,item.id),label=item.kind.equals("stroke")?"Ink stroke":com.notemaker.core.ShapeCatalog.name(String.valueOf(item.content.get("shape")));
                entities.add(item.ref());labels.add(label+" · "+item.id.substring(0,Math.min(8,item.id.length()))+(owner==null?"":" · Locked by "+owner));
            }
        }
        if(labels.isEmpty()){Toast.makeText(app,"There are no items on this layer and page",Toast.LENGTH_SHORT).show();return;}
        boolean[] checked=new boolean[labels.size()];for(int i=0;i<entities.size();i++)for(Map<String,Object> selected:app.selectedEntities)if(entities.get(i).equals(selected))checked[i]=true;
        new AlertDialog.Builder(app).setTitle("Select entities").setMultiChoiceItems(labels.toArray(new String[0]),checked,(dialog,index,value)->checked[index]=value)
                .setNegativeButton("Cancel",null).setPositiveButton("Select",(dialog,which)->{ArrayList<Map<String,Object>> chosen=new ArrayList<>();for(int i=0;i<checked.length;i++)if(checked[i])chosen.add(entities.get(i));if(app.selectionSession!=null){app.deselect();app.selecting=true;}if(chosen.isEmpty())app.selecting=false;else app.startSelection(chosen);}).show();
    }

    private String leaseActor(String kind,String id){Object raw=app.leases.get(app.page+":"+kind+":"+id);return raw instanceof Map?String.valueOf(com.notemaker.core.Json.map(raw).getOrDefault("actor","another session")):null;}

    private LinearLayout templatePanel() {
        LinearLayout box = popupBody("Page templates", "Adds a page-sized background guide behind your ink.");
        menuAction(box, "Ruled", true, () -> app.insertTemplate("ruled"));
        menuAction(box, "Graph", true, () -> app.insertTemplate("grid"));
        menuAction(box, "Dot grid", true, () -> app.insertTemplate("dots"));
        menuAction(box, "Planner · checklist", true, () -> app.insertTemplate("planner"));
        return box;
    }

    private void applyPenPreset(int color, int width, String brush, int material) {
        if(app.selecting||app.selectionSession!=null||!app.selectedEntities.isEmpty())app.deselect();
        selectedTool="pen";app.erase=false;app.pan=false;app.fillTapMode=false;app.resetDrawingGesture();
        app.color = color;
        app.width = Math.max(1, Math.min(30, width));
        app.brush = brush;
        app.material=Math.max(0,Math.min(100,material));
        app.getSharedPreferences("settings",0).edit().putInt("penMaterial",app.material).apply();
        app.update();
        dismissPopup();
    }

    private void savePenPreset() {
        EditText name = new EditText(app);
        name.setSingleLine(true);
        name.setHint("Preset name");
        LinearLayout form = column();
        form.setPadding(dp(24), dp(8), dp(24), dp(8));
        form.addView(name);
        new AlertDialog.Builder(app).setTitle("Save pen preset").setView(form)
                .setNegativeButton("Cancel", null).setPositiveButton("Save", (dialog, which) -> {
                    String label = name.getText().toString().trim();
                    if (label.isEmpty()) label = "Preset " + (app.getSharedPreferences("settings", 0).getString("penPresets", "").length() + 1);
                    android.content.SharedPreferences prefs = app.getSharedPreferences("settings", 0);
                    org.json.JSONArray list;
                    try { list = new org.json.JSONArray(prefs.getString("penPresets", "")); }
                    catch (Exception e) { list = new org.json.JSONArray(); }
                    list.put(MainActivity.obj("name", label, "color", app.color, "width", (int) app.width, "brush", app.brush, "material", app.material));
                    prefs.edit().putString("penPresets", list.toString()).apply();
                    dismissPopup();
                }).show();
    }

    void dismissPopup() { if (popup != null) { popup.dismiss(); popup = null; } }

    void showConflictPanel() { dismissPopup(); if (connection != null && !app.conflicts.isEmpty()) showPopup(conflictsPanel(), connection, false); }
    void showSelectionPanel() { dismissPopup(); if (tools.get("select") != null) showPopup(selectionPanel(), tools.get("select"), false); }
    private void showPopup(LinearLayout body, View anchor, boolean fromRight) {
        if(root.getWidth()<=0||!root.isAttachedToWindow()){root.post(()->showPopup(body,anchor,fromRight));return;}
        dismissPopup();
        int[] rootPosition = new int[2]; root.getLocationOnScreen(rootPosition);
        int[] position = new int[2]; anchor.getLocationOnScreen(position);
        boolean top=anchor instanceof Tool&&((Tool)anchor).top;
        int left=rootPosition[0]+root.getPaddingLeft()+dp(8),right=rootPosition[0]+root.getWidth()-root.getPaddingRight()-dp(8);
        int topLimit=rootPosition[1]+root.getPaddingTop()+dp(8),bottom=rootPosition[1]+root.getHeight()-root.getPaddingBottom()-dp(8);
        int panelLeft=top?left:Math.max(left,position[0]+anchor.getWidth()+dp(8));
        int requested=body.getTag() instanceof Integer?(Integer)body.getTag():320;
        int popupWidth = Math.min(dp(requested), right-panelLeft);
        if(popupWidth<dp(220)){panelLeft=left;popupWidth=Math.min(dp(requested),right-left);}
        if(top)topLimit=Math.min(bottom-dp(48),position[1]+anchor.getHeight()+dp(8));
        body.measure(View.MeasureSpec.makeMeasureSpec(popupWidth, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
        int naturalHeight=body.getMeasuredHeight(),maxHeight=Math.max(dp(48),bottom-topLimit),popupHeight=Math.min(naturalHeight,maxHeight);
        View content=body;
        if(naturalHeight>maxHeight){ScrollView scroll=new ScrollView(app);scroll.setFillViewport(false);scroll.setVerticalScrollBarEnabled(true);scroll.addView(body,new ScrollView.LayoutParams(-1,-2));content=scroll;}
        FrameLayout shell=new FrameLayout(app){
            final Paint border=new Paint(Paint.ANTI_ALIAS_FLAG);
            @Override protected void dispatchDraw(Canvas canvas){
                super.dispatchDraw(canvas);border.setColor(INK);border.setStyle(Paint.Style.STROKE);border.setStrokeWidth(dp(1));
                float inset=dp(1)/2f;canvas.drawRoundRect(inset,inset,getWidth()-inset,getHeight()-inset,dp(8),dp(8),border);
            }
        };
        shell.setPadding(dp(1),dp(1),dp(1),dp(1));shell.setBackground(surface(Color.WHITE,INK,8,0));shell.setClipToOutline(true);shell.addView(content,new FrameLayout.LayoutParams(-1,-1));
        popup = new PopupWindow(shell, popupWidth, popupHeight, true);
        popup.setBackgroundDrawable(new android.graphics.drawable.ColorDrawable(Color.TRANSPARENT));
        popup.setOutsideTouchable(true);
        popup.setElevation(0);
        popup.setAnimationStyle(0);
        int x=top?Math.max(left,Math.min(position[0]+anchor.getWidth()-popupWidth,right-popupWidth)):panelLeft;
        int y=top?topLimit:Math.max(topLimit,Math.min(position[1],bottom-popupHeight));
        popupAnchor=anchor instanceof Tool?(Tool)anchor:null;
        if(popupAnchor!=null){popupAnchor.open=true;popupAnchor.invalidate();}
        LinearLayout displayedLayers=layerList;
        StrokeSample displayedSample=sample;
        popup.setOnDismissListener(()->{
            if(popupAnchor!=null){popupAnchor.open=false;popupAnchor.invalidate();popupAnchor=null;}
            if(layerList==displayedLayers){layerList=null;previews.clear();}
            if(sample==displayedSample)sample=null;
        });
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
        boolean selected,open,top;
        final Paint marker=new Paint(Paint.ANTI_ALIAS_FLAG);
        Tool(String kind, String name, String description) {
            super(app);
            setOrientation(VERTICAL); setGravity(Gravity.CENTER);
            setContentDescription(description);
            icon = new Icon(kind);
            label = text(name, 11, INK, false);
            label.setGravity(Gravity.CENTER);
            label.setSingleLine(true);label.setEllipsize(TextUtils.TruncateAt.END);
            label.setVisibility(showToolLabels?View.VISIBLE:View.GONE);
            addView(icon, new LinearLayout.LayoutParams(dp(40), dp(40)));
            addView(label,new LinearLayout.LayoutParams(-1,-2));
            setPadding(dp(2),0,dp(2),dp(6));setBackgroundColor(PAPER);
        }
        @Override protected void dispatchDraw(Canvas canvas){
            super.dispatchDraw(canvas);marker.setColor(INK);
            if(selected)canvas.drawCircle(getWidth()/2f,getHeight()-dp(4),dp(2.5f),marker);
            if(open){Path triangle=new Path();if(top){triangle.moveTo(getWidth()/2f-dp(5),getHeight());triangle.lineTo(getWidth()/2f,getHeight()-dp(6));triangle.lineTo(getWidth()/2f+dp(5),getHeight());}
                else{triangle.moveTo(getWidth(),getHeight()/2f-dp(5));triangle.lineTo(getWidth()-dp(6),getHeight()/2f);triangle.lineTo(getWidth(),getHeight()/2f+dp(5));}triangle.close();canvas.drawPath(triangle,marker);}
        }
        void active(boolean value) {
            if (selected == value) return;
            selected = value;
            setSelected(value);invalidate();
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
            LinkedHashMap<String,Map<String,Object>> settled=new LinkedHashMap<>();
            for(Map.Entry<String,Map<String,Object>> entry:app.model.strokes.entrySet()){
                Map<String,Object> stroke=entry.getValue();if(!entry.getKey().equals(app.canvas.current)&&layer.equals(stroke.get("layer"))&&((Number)stroke.get("page")).intValue()==app.page&&!Boolean.FALSE.equals(app.active.get(entry.getKey()))){Map<String,Object> preview=new LinkedHashMap<>(stroke);preview.put("width",Math.max(((Number)stroke.get("width")).doubleValue(),dp(1.4f)/scale));settled.put(entry.getKey(),preview);}
            }
            raster.draw(c,settled,image->{image.clipRect(1,dp(16),w-1,h-1);image.translate((w-1000*scale)/2,dp(16));image.scale(scale,scale);},(image,stroke)->{
                com.notemaker.core.Render.drawStroke(stroke,new com.notemaker.cli.Png(image));
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
        String kind;
        int color = INK;
        int accentColor = INK;
        final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        Icon(String kind) { super(app); this.kind = kind; }
        protected void onDraw(Canvas canvas) {
            canvas.save();
            float size = Math.min(getWidth(), getHeight()) * .62f;
            canvas.translate((getWidth() - size) / 2, (getHeight() - size) / 2);
            canvas.scale(size / 24, size / 24);
            paint.setColor(INK); paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(1.7f);
            paint.setStrokeCap(Paint.Cap.ROUND); paint.setStrokeJoin(Paint.Join.ROUND);
            switch (kind) {
                case "agent":
                    canvas.drawRoundRect(4,7,20,21,3,3,paint);
                    path(canvas,12,7,12,4);canvas.drawCircle(12,2.5f,1.5f,paint);
                    path(canvas,2,11,2,17);path(canvas,22,11,22,17);
                    path(canvas,9,17,15,17);
                    paint.setStyle(Paint.Style.FILL);
                    canvas.drawCircle(8.5f,12,1.5f,paint);canvas.drawCircle(15.5f,12,1.5f,paint);
                    break;
                case "undo": path(canvas,8,5,3,10,8,15);path(canvas,3,10,14,10);canvas.drawArc(new RectF(9,10,21,22),270,150,false,paint);break;
                case "redo": path(canvas,16,5,21,10,16,15);path(canvas,21,10,10,10);canvas.drawArc(new RectF(3,10,15,22),270,-150,false,paint);break;
                case "check":path(canvas,4,12,10,18,21,5);break;
                case "layers":path(canvas,2,10,12,3,22,10,12,17,2,10);path(canvas,3,15,12,22,21,15);break;
                case "template":canvas.drawRect(3,3,21,21,paint);for(int y=8;y<22;y+=5)path(canvas,6,y,18,y);break;
                case "insert":canvas.drawRect(3,3,21,21,paint);canvas.drawCircle(8,8,2,paint);path(canvas,4,19,11,12,15,16,19,11,21,13);break;
                case "fountain":path(canvas,12,2,5,15,8,20,16,20,19,15,12,2);path(canvas,12,3,12,12);canvas.drawCircle(12,14,2,paint);break;
                case "pencil":path(canvas,5,21,5,10,12,2,19,10,19,21,5,21);path(canvas,5,10,9,13,12,10,15,13,19,10);path(canvas,12,11,12,21);break;
                case "marker":path(canvas,4,21,4,12,20,12,20,21,4,21);path(canvas,7,12,7,6,17,3,17,12);break;
                case "brush":{Path brush=new Path();brush.moveTo(9,16);brush.cubicTo(2,11,13,8,11,2);brush.cubicTo(22,10,17,16,15,16);canvas.drawPath(brush,paint);canvas.drawRect(6,17,18,22,paint);break;}
                case "pen": path(canvas, 4, 20, 6, 13, 16, 3, 21, 8, 11, 18, 4, 20); path(canvas, 14, 5, 19, 10); break;
                case "eraser": path(canvas, 3, 14, 13, 4, 21, 12, 13, 20, 9, 20, 3, 14); path(canvas, 7, 10, 15, 18); path(canvas, 13, 20, 22, 20); break;
                case "move": path(canvas, 12, 2, 12, 22); path(canvas, 2, 12, 22, 12); path(canvas, 8, 6, 12, 2, 16, 6); path(canvas, 8, 18, 12, 22, 16, 18); path(canvas, 6, 8, 2, 12, 6, 16); path(canvas, 18, 8, 22, 12, 18, 16); break;
                case "history": canvas.drawArc(new RectF(4, 4, 21, 21), 210, 290, false, paint); path(canvas, 3, 3, 3, 9, 9, 9); path(canvas, 12, 7, 12, 13, 16, 15); break;
                case "document": case "page": path(canvas, 5, 2, 15, 2, 20, 7, 20, 22, 5, 22, 5, 2); path(canvas, 15, 2, 15, 7, 20, 7); path(canvas, 9, 12, 16, 12); path(canvas, 9, 16, 16, 16); break;
                case "text": path(canvas, 5, 5, 19, 5);path(canvas,12,5,12,20);path(canvas,8,20,16,20);break;
                case "select": paint.setPathEffect(new DashPathEffect(new float[]{3,2},0));canvas.drawRect(4,4,20,20,paint);paint.setPathEffect(null);path(canvas,12,4,12,20);path(canvas,4,12,20,12);break;
                case "fill": { Path fill=new Path();fill.moveTo(12,3);fill.cubicTo(9,8,4,12,4,16);fill.cubicTo(4,22,20,22,20,16);fill.cubicTo(20,12,15,8,12,3);canvas.drawPath(fill,paint);break; }
                case "shapes":canvas.drawRect(3,4,13,14,paint);canvas.drawOval(11,10,21,20,paint);path(canvas,4,21,10,13,16,21,4,21);break;
                case "rect":canvas.drawRect(4,5,20,19,paint);break;
                case "ellipse":canvas.drawOval(4,5,20,19,paint);break;
                case "circle":canvas.drawCircle(12,12,8,paint);break;
                case "rectangle":canvas.drawRect(4,5,20,19,paint);break;
                case "line":path(canvas,4,19,20,5);break;
                case "arrow":path(canvas,4,19,19,5,19,12);path(canvas,19,5,12,5);break;
                case "triangle":path(canvas,12,4,21,20,3,20,12,4);break;
                case "triangle45":path(canvas,4,20,20,20,20,4,4,20);path(canvas,15,20,20,15);break;
                case "triangle60":path(canvas,12,3,21,20,3,20,12,3);break;
                case "triangle90":path(canvas,4,3,4,20,21,20,4,3);path(canvas,4,16,8,16,8,20);break;
                case "diamond":path(canvas,12,3,21,12,12,21,3,12,12,3);break;
                case "rhombus":path(canvas,12,3,21,12,12,21,3,12,12,3);break;
                case "pentagon":poly(canvas,5,12);break;
                case "hexagon":poly(canvas,6,12);break;
                case "regularHexagon":poly(canvas,6,12);break;
                case "regularPolygon":poly(canvas,8,11);break;
                case "star":poly(canvas,10,12);break;
                case "cube":path(canvas,4,8,12,3,20,8,12,13,4,8);path(canvas,4,8,4,18,12,22,12,13);path(canvas,20,8,20,18,12,22);break;
                case "pyramid":path(canvas,12,3,3,20,21,20,12,3);path(canvas,12,3,12,20);path(canvas,3,20,15,12);path(canvas,21,20,9,12);break;
                case "cylinder":canvas.drawOval(5,3,19,9,paint);path(canvas,5,6,5,18);path(canvas,19,6,19,18);canvas.drawArc(new RectF(5,15,19,21),0,180,false,paint);break;
                case "cone":path(canvas,12,3,4,19);path(canvas,12,3,20,19);canvas.drawOval(4,15,20,21,paint);break;
                case "oily": case "neoBrush": case "charcoal": case "latinCalligraphy": case "asiaCalligraphy": case "square":
                    path(canvas,4,20,6,13,16,3,21,8,11,18,4,20);path(canvas,14,5,19,10);break;
                case "sphere":canvas.drawOval(4,4,20,20,paint);path(canvas,4,13,9,10,15,10,20,13);break;
                case "arrowLine":path(canvas,4,19,19,5,19,12);path(canvas,19,5,12,5);break;
                case "trapezoid":path(canvas,7,4,17,4,21,20,3,20,7,4);break;
                case "wave": case "waveLine": { Path wave=new Path();wave.moveTo(3,15);wave.cubicTo(7,2,11,2,13,13);wave.cubicTo(15,24,19,21,21,8);canvas.drawPath(wave,paint);break; }
                case "curve": { Path curve=new Path();curve.moveTo(4,20);curve.cubicTo(3,3,20,3,20,20);canvas.drawPath(curve,paint);break; }
                case "polyline": path(canvas,3,18,8,6,14,17,20,5);break;
                case "eye": case "hidden":
                    Path eye = new Path(); eye.moveTo(2, 12); eye.quadTo(12, 0, 22, 12); eye.quadTo(12, 24, 2, 12); canvas.drawPath(eye, paint); canvas.drawCircle(12, 12, 3, paint);
                    if (kind.equals("hidden")) path(canvas, 3, 3, 21, 21); break;
                case "lock": canvas.drawRoundRect(5, 10, 19, 21, 2, 2, paint); canvas.drawArc(new RectF(8, 2, 16, 17), 180, 180, false, paint); path(canvas, 12, 14, 12, 17); break;
                case "collapse": path(canvas, 8, 5, 15, 12, 8, 19); path(canvas, 20, 5, 20, 19); break;
                case "expand": path(canvas, 16, 5, 9, 12, 16, 19); path(canvas, 4, 5, 4, 19); break;
                case "plus": path(canvas, 12, 4, 12, 20); path(canvas, 4, 12, 20, 12); break;
                case "more": paint.setStyle(Paint.Style.FILL); for (int x = 5; x <= 19; x += 7) canvas.drawCircle(x, 12, 1.7f, paint); break;
                default: poly(canvas,6,11); break;
            }
            if (kind.equals("pen") || kind.equals("oily") || kind.equals("neoBrush") || kind.equals("charcoal") || kind.equals("latinCalligraphy") || kind.equals("asiaCalligraphy") || kind.equals("square") || kind.equals("pencil") || kind.equals("marker") || kind.equals("brush") || kind.equals("fountain")) {
                paint.setColor(accentColor); paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(2.5f);
                switch (kind) {
                    case "pen": case "oily": case "neoBrush": case "charcoal": case "latinCalligraphy": case "asiaCalligraphy": case "square": path(canvas,16,3,21,8,19,10); break;
                    case "pencil": path(canvas,12,2,15,5,12,8,9,5,12,2); break;
                    case "marker": path(canvas,7,6,17,3,17,8,7,10); break;
                    case "brush": canvas.drawCircle(12,7,2,paint); break;
                    case "fountain": canvas.drawCircle(12,14,2,paint); break;
                }
            }
            canvas.restore();
        }
        void path(Canvas canvas, float... coordinates) {
            Path path = new Path(); path.moveTo(coordinates[0], coordinates[1]);
            for (int i = 2; i < coordinates.length; i += 2) path.lineTo(coordinates[i], coordinates[i + 1]);
            canvas.drawPath(path, paint);
        }
        void poly(Canvas canvas,int count,float radius){Path path=new Path();for(int i=0;i<count;i++){float r=kind.equals("star")&&i%2==1?radius*.45f:radius;double a=-Math.PI/2+i*Math.PI*2/count;float x=12+(float)Math.cos(a)*r,y=12+(float)Math.sin(a)*r;if(i==0)path.moveTo(x,y);else path.lineTo(x,y);}path.close();canvas.drawPath(path,paint);}
    }
}
