package com.notemaker;

import android.app.Dialog;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.*;
import android.widget.*;
import org.json.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;

/** Server-backed popup. Closing the view only closes its subscription, never the run. */
final class AgentUi {
    private final MainActivity app;
    private final Handler ui = new Handler(Looper.getMainLooper());
    private final ExecutorService requests = Executors.newSingleThreadExecutor();
    private final Map<String, Item> items = new LinkedHashMap<>();
    private final ArrayList<JSONObject> incoming = new ArrayList<>();
    private Dialog dialog;
    private LinearLayout body, messages;
    private ScrollView scroll;
    private TextView status;
    private EditText input;
    private Button send, stop;
    private String base, proxyToken, chat;
    private volatile int generation;
    private volatile HttpURLConnection stream;
    private boolean rendering, busy;
    private JSONObject pending;
    private String lastStatus = "";

    AgentUi(MainActivity app) { this.app = app; }
    private int dp(int v) { return Math.round(v * app.getResources().getDisplayMetrics().density); }
    private LinearLayout column() { LinearLayout v = new LinearLayout(app); v.setOrientation(1); return v; }
    private TextView text(String value, int size) {
        TextView v = new TextView(app); v.setText(value); v.setTextSize(size); v.setTextColor(0xff202521);
        v.setPadding(dp(4), dp(6), dp(4), dp(6)); return v;
    }
    private Button button(String title, Runnable action) {
        Button v = new Button(app); v.setText(title); v.setAllCaps(false); v.setContentDescription(title);
        v.setOnClickListener(w -> action.run()); return v;
    }
    private void cancelStream() {
        generation++;
        rendering = false;
        HttpURLConnection connection = stream;
        stream = null;
        if (connection != null) requests.execute(connection::disconnect);
        synchronized (incoming) { incoming.clear(); }
    }
    private String draftKey() { return "agent-draft:" + base + ":" + chat; }
    private String pendingKey() { return "agent-pending:" + base + ":" + chat; }
    private void saveDraft() {
        if (chat != null && input != null) {
            if (pending != null && !app.getSharedPreferences("settings", 0).contains(pendingKey())) return;
            app.getSharedPreferences("settings", 0).edit().putString(draftKey(), input.getText().toString()).apply();
        }
    }
    void dismiss() { if (dialog != null) dialog.dismiss(); }
    boolean showing() { return dialog != null && dialog.isShowing(); }
    void destroy() { dismiss(); requests.shutdownNow(); }
    void show() {
        if (dialog != null && dialog.isShowing()) return;
        try {
            if (!app.proxyUrl.isEmpty()) { base=app.proxyUrl.replaceAll("/+$", ""); proxyToken=app.proxyToken; }
            else {
                ReplicaService.ensure(app);
                File descriptor=new File(app.getFilesDir(),"proxy.json");
                JSONObject proxy=new JSONObject(new String(java.nio.file.Files.readAllBytes(descriptor.toPath()),StandardCharsets.UTF_8));
                base=proxy.getString("url").replaceAll("/+$", ""); proxyToken=proxy.getString("token");
            }
        } catch(Exception e) {
            Toast.makeText(app, "Cannot connect to the configured proxy: " + e.getMessage(), Toast.LENGTH_LONG).show(); return;
        }
        dialog = new Dialog(app);
        dialog.setTitle("Agent");
        body = column(); body.setPadding(dp(16), dp(12), dp(16), dp(12)); body.setBackgroundColor(Color.WHITE);
        // A full-window transparent host receives IME insets even on BOOX. Only
        // the centered white panel is visible, and it fits the remaining space.
        FrameLayout frame = new FrameLayout(app);
        final LinearLayout panel = body;
        panel.setClickable(true);
        frame.setPadding(dp(12), dp(12), dp(12), dp(12));
        frame.addView(panel, new FrameLayout.LayoutParams(-1, -1, Gravity.CENTER));
        frame.setOnClickListener(v -> dismiss());
        frame.addOnLayoutChangeListener((v,l,t,r,b,ol,ot,or,ob) -> resizePopup(frame, panel));
        if (Build.VERSION.SDK_INT >= 30) frame.setOnApplyWindowInsetsListener((v, insets) -> {
            Insets safe = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout() | WindowInsets.Type.ime());
            frame.setPadding(safe.left + dp(12), safe.top + dp(12), safe.right + dp(12), safe.bottom + dp(12));
            resizePopup(frame, panel);
            return insets;
        });
        dialog.setContentView(frame);
        dialog.setOnDismissListener(d -> { saveDraft(); cancelStream(); dialog = null; });
        dialog.show();
        Window window = dialog.getWindow();
        if (window != null) {
            window.setBackgroundDrawable(new ColorDrawable(Color.TRANSPARENT));
            window.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);
            window.setDimAmount(.32f);
            window.setLayout(-1, -1);
            if (Build.VERSION.SDK_INT >= 30) {
                window.setDecorFitsSystemWindows(false);
                window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
            } else window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        }
        frame.requestApplyInsets();
        hub(0, true);
    }
    private void resizePopup(FrameLayout frame, LinearLayout panel) {
        if (frame.getHeight() == 0) return;
        int width = Math.max(1, Math.min(dp(640), frame.getWidth() - frame.getPaddingLeft() - frame.getPaddingRight()));
        int height = Math.max(1, Math.min((int)(frame.getHeight() * .85), frame.getHeight() - frame.getPaddingTop() - frame.getPaddingBottom()));
        FrameLayout.LayoutParams params = (FrameLayout.LayoutParams)panel.getLayoutParams();
        if (params.width != width || params.height != height) {
            params.width = width; params.height = height; panel.setLayoutParams(params);
        }
    }
    private void clear(String heading) {
        saveDraft(); cancelStream(); chat = null; input = null; pending = null;
        body.removeAllViews();
        LinearLayout row = new LinearLayout(app);
        row.addView(text(heading, 22), new LinearLayout.LayoutParams(0, -2, 1));
        row.addView(button("Close", this::dismiss)); body.addView(row);
        status = text("", 13); body.addView(status);
    }
    private interface Work { JSONObject run() throws Exception; }
    private interface Result { void accept(JSONObject result) throws Exception; }
    private void request(Work work, Result done) {
        final int token = generation;
        requests.execute(() -> {
            try {
                JSONObject result = work.run();
                ui.post(() -> { if (token == generation && dialog != null) {
                    try { done.accept(result); } catch (Exception e) { status.setText(e.getMessage()); }
                }});
            } catch (Exception e) {
                ui.post(() -> { if (token == generation && dialog != null) {
                    status.setText("Connection/request failed: " + e.getMessage());
                    if (send != null && chat != null) { send.setEnabled(true); if(pending!=null)send.setText("Retry send"); }
                }});
            }
        });
    }
    private JSONObject http(String url, JSONObject data) throws Exception {
        HttpURLConnection c = (HttpURLConnection)new URL(url).openConnection();
        c.setConnectTimeout(5000); c.setReadTimeout(10000);
        c.setRequestProperty("Authorization", "Bearer " + proxyToken);
        try {
            if (data != null) {
                c.setRequestMethod("POST"); c.setDoOutput(true); c.setRequestProperty("Content-Type", "application/json");
                try (OutputStream out = c.getOutputStream()) { out.write(data.toString().getBytes(StandardCharsets.UTF_8)); }
            }
            int code = c.getResponseCode();
            InputStream source = code < 400 ? c.getInputStream() : c.getErrorStream();
            if (source == null) throw new IOException("HTTP " + code);
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            try (InputStream in = source) { byte[] buffer = new byte[4096]; int n; while ((n=in.read(buffer))!=-1) bytes.write(buffer,0,n); }
            JSONObject result = new JSONObject(bytes.toString("UTF-8"));
            if (code >= 400) throw new IOException(result.optString("error", "HTTP " + code));
            return result;
        } finally { c.disconnect(); }
    }
    private void hub(int offset, boolean recent) {
        clear(recent ? "Agent" : "All chats");
        body.addView(button("New chat", this::newChat));
        ScrollView listScroll = new ScrollView(app); LinearLayout list = column(); listScroll.addView(list);
        body.addView(listScroll, new LinearLayout.LayoutParams(-1, 0, 1));
        status.setText(recent ? "Five most recent chats" : "Chat history");
        final String url = base + "/api/chats?limit=" + (recent ? 5 : 20) + "&offset=" + offset;
        request(() -> http(url, null), result -> {
            JSONArray chats = result.getJSONArray("chats");
            if (chats.length() == 0) list.addView(text("No chats yet", 16));
            for (int i=0;i<chats.length();i++) {
                JSONObject item = chats.getJSONObject(i);
                list.addView(button(item.getString("title") + " · " + item.getString("status"), () -> conversation(item)));
            }
            if (recent) body.addView(button("More chats", () -> hub(0, false)));
            else {
                if (offset > 0) body.addView(button("Previous chats", () -> hub(Math.max(0, offset-20), false)));
                if (result.optBoolean("more")) body.addView(button("Next chats", () -> hub(offset+20, false)));
                body.addView(button("Recent chats", () -> hub(0, true)));
            }
        });
        body.addView(button("Refresh", () -> hub(offset, recent)));
    }
    private Spinner spinner(String description, String[] values) {
        Spinner v = new Spinner(app); v.setContentDescription(description);
        ArrayAdapter<String> adapter = new ArrayAdapter<>(app, android.R.layout.simple_spinner_dropdown_item, values);
        v.setAdapter(adapter); body.addView(text(description, 14)); body.addView(v); return v;
    }
    private void newChat() {
        clear("New chat"); status.setText("Loading available agents…");
        final String url = base + "/api/chats/options";
        request(() -> http(url, null), result -> {
            status.setText("Choose an agent for this conversation");
            JSONArray harnesses = result.getJSONArray("harnesses"), models = result.getJSONArray("models");
            String[] harnessNames = new String[harnesses.length()], modelNames = new String[models.length()];
            for (int i=0;i<harnessNames.length;i++) harnessNames[i]=harnesses.getString(i);
            for (int i=0;i<modelNames.length;i++) modelNames[i]=models.getJSONObject(i).getString("provider")+" / "+models.getJSONObject(i).getString("model");
            Spinner harness = spinner("Harness", harnessNames), model = spinner("Model", modelNames), effort = spinner("Effort", new String[]{"medium"});
            model.setOnItemSelectedListener(new android.widget.AdapterView.OnItemSelectedListener() {
                public void onNothingSelected(android.widget.AdapterView<?> parent) {}
                public void onItemSelected(android.widget.AdapterView<?> parent, View view, int position, long id) {
                    JSONArray choices = models.optJSONObject(position).optJSONArray("efforts");
                    String[] names = new String[choices.length()]; int selected=0;
                    for (int i=0;i<names.length;i++) { names[i]=choices.optString(i); if (names[i].equals("medium")) selected=i; }
                    effort.setAdapter(new ArrayAdapter<>(app, android.R.layout.simple_spinner_dropdown_item, names)); effort.setSelection(selected);
                }
            });
            final String identity = UUID.randomUUID().toString();
            Button create = button("Start chat", () -> {
                JSONObject selected = models.optJSONObject(model.getSelectedItemPosition());
                JSONObject config = MainActivity.obj("id",identity,"harness",harness.getSelectedItem().toString(),
                        "provider",selected.optString("provider"),"model",selected.optString("model"),"effort",effort.getSelectedItem().toString());
                final String target = base + "/api/chats";
                request(() -> http(target, config), this::conversation);
            });
            body.addView(create);
        });
        body.addView(button("Back to chats", () -> hub(0, true)));
    }
    private void conversation(JSONObject data) {
        clear(data.optString("title", "Chat")); chat = data.optString("id"); items.clear();
        body.addView(button("Back to chats", () -> hub(0, true)));
        body.addView(text(data.optString("harness") + " / " + data.optString("model") + " / " + data.optString("effort"), 12));
        scroll = new ScrollView(app); messages = column(); scroll.addView(messages);
        body.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        input = new EditText(app); input.setHint("Message agent"); input.setContentDescription("Message agent"); input.setMinLines(1); input.setMaxLines(4);
        input.setInputType(android.text.InputType.TYPE_CLASS_TEXT | android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE | android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        input.setText(app.getSharedPreferences("settings", 0).getString(draftKey(), "")); body.addView(input);
        try { String saved=app.getSharedPreferences("settings",0).getString(pendingKey(),""); if(!saved.isEmpty())pending=new JSONObject(saved); } catch(Exception ignored) {}
        LinearLayout actions = new LinearLayout(app); send=button("Send", this::send); stop=button("Stop", this::stop);
        actions.addView(send, new LinearLayout.LayoutParams(0,-2,1)); actions.addView(stop, new LinearLayout.LayoutParams(0,-2,1)); body.addView(actions);
        setStatus(data.optString("status"), data.optString("status"));
        if (pending != null) { input.setText(pending.optString("text")); send.setText("Retry send"); send.setEnabled(true); }
        subscribe(generation, base, chat);
    }
    private void setStatus(String state, String detail) {
        busy=state.equals("running"); lastStatus=detail; status.setText(detail); stop.setEnabled(busy); send.setEnabled(!busy || pending!=null);
    }
    private void send() {
        if (pending == null) {
            String value = input.getText().toString().trim(); if(value.isEmpty())return;
            pending=MainActivity.obj("request",UUID.randomUUID().toString(),"text",value,"document",app.document,"session",app.appSessionId);
            app.getSharedPreferences("settings",0).edit().putString(pendingKey(),pending.toString()).apply();
        }
        final JSONObject message = pending; final String key=pendingKey(), draft=draftKey(), url=base+"/api/chats/"+chat+"/messages";
        final int token=generation;
        setStatus("running","Sending…"); send.setEnabled(false); input.setEnabled(false);
        request(() -> {
            try { JSONObject result=http(url,message); app.getSharedPreferences("settings",0).edit().remove(key).remove(draft).apply(); return result; }
            finally { ui.post(() -> { if(token==generation && input!=null)input.setEnabled(true); }); }
        }, result -> { pending=null; input.setText(""); send.setText("Send"); send.setEnabled(!busy); });
    }
    private void stop() {
        final String url=base+"/api/chats/"+chat+"/stop";
        request(() -> http(url,new JSONObject()), result -> status.setText(result.optBoolean("stopping") ? "Stopping…" : "Finished"));
    }
    private void subscribe(int token, String host, String identity) {
        Thread reader = new Thread(() -> {
            long after=0;
            while(token==generation) {
                HttpURLConnection c=null;
                try {
                    c=(HttpURLConnection)new URL(host+"/api/chats/"+identity+"/stream?after="+after).openConnection();
                    c.setRequestProperty("Authorization", "Bearer " + proxyToken);
                    c.setConnectTimeout(5000); c.setReadTimeout(10000); stream=c;
                    try(BufferedReader lines=new BufferedReader(new InputStreamReader(c.getInputStream(),StandardCharsets.UTF_8))) {
                        ui.post(() -> { if(token==generation && dialog!=null)status.setText(lastStatus); });
                        String line;
                        while(token==generation && (line=lines.readLine())!=null) {
                            JSONObject event=new JSONObject(line); if(!event.has("seq"))continue;
                            long seq=event.getLong("seq"); if(seq<=after)continue;
                            after=seq;
                            synchronized(incoming) { if(token!=generation)break; incoming.add(event.getJSONObject("event")); }
                            ui.post(() -> { if(token==generation && !rendering) { rendering=true; ui.postDelayed(() -> flush(token),250); } });
                        }
                    }
                } catch(Exception e) {
                    ui.post(() -> { if(token==generation && dialog!=null)status.setText("Disconnected · reconnecting. Draft kept on device."); });
                    try { Thread.sleep(1500); } catch(InterruptedException ignored) { return; }
                } finally { if(c!=null)c.disconnect(); }
            }
        },"agent-chat-stream"); reader.setDaemon(true); reader.start();
    }
    private void flush(int token) {
        rendering=false; if(token!=generation || dialog==null)return;
        ArrayList<JSONObject> events;
        synchronized(incoming) { events=new ArrayList<>(incoming); incoming.clear(); }
        boolean bottom=scroll.getChildCount()==0 || scroll.getScrollY()+scroll.getHeight() >= messages.getHeight()-dp(60);
        Set<Item> changed=new HashSet<>();
        for(JSONObject event:events) {
            String kind=event.optString("kind"), id=event.optString("id");
            if(kind.equals("status")) { setStatus(event.optString("status"),event.optString("text")); continue; }
            if(kind.equals("error")) { messages.addView(text(event.optString("text"),14)); continue; }
            if(kind.equals("user") && pending!=null && id.equals(pending.optString("request"))) {
                pending=null; input.setText(""); send.setText("Send");
                app.getSharedPreferences("settings",0).edit().remove(pendingKey()).remove(draftKey()).apply();
            }
            Item item=items.get(id);
            if(item==null && (kind.equals("user") || kind.equals("assistant") || kind.equals("tool"))) {
                item=new Item(kind,event); items.put(id,item); messages.addView(item.view);
            }
            if(item==null)continue;
            if(kind.equals("delta"))item.value.append(event.optString("text"));
            else { item.value.setLength(0); item.value.append(event.optString("text")); }
            if(event.has("status"))item.state=event.optString("status");
            changed.add(item);
        }
        for(Item item:changed)item.refresh();
        if(bottom)scroll.post(() -> scroll.fullScroll(View.FOCUS_DOWN));
    }
    private class Item {
        final LinearLayout view=column(); final TextView heading, detail;
        final StringBuilder value=new StringBuilder(); final String kind,summary,arguments;
        String state=""; boolean expanded;
        Item(String kind,JSONObject event) {
            this.kind=kind;
            ToolDisplay tool = new ToolDisplay(event.optString("name"), event.optString("args"));
            summary=tool.summary; arguments=tool.details;
            heading=text(kind.equals("user")?"You":"Agent",14); detail=text("",16); detail.setTextIsSelectable(true);
            view.setPadding(dp(8),dp(6),dp(8),dp(10)); view.addView(heading); view.addView(detail);
            if(kind.equals("tool")) {
                detail.setTypeface(Typeface.MONOSPACE); detail.setTextSize(13);
                heading.setContentDescription("Expand tool activity"); heading.setMinHeight(dp(48)); heading.setOnClickListener(v -> { expanded=!expanded; refresh(); });
            }
        }
        void refresh() {
            if(kind.equals("tool")) {
                heading.setText((expanded?"▾ ":"▸ ")+summary+" · "+state); detail.setVisibility(expanded?View.VISIBLE:View.GONE);
                if(expanded)detail.setText(arguments+(value.length()==0 ? "" : "\n\nOutput\n"+value));
            } else if(!value.toString().contentEquals(detail.getText()))detail.setText(value);
        }
    }
}
