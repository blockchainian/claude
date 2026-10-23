# 事件详情里的内联图片：设计摘要

工具输出和消息里的图片，现在一律显示成 `[image image/png]`。本设计用 kitty 图形协议在事件详情里画图：iTerm2、kitty、Ghostty、WezTerm 都支持这个协议，所以不针对哪一个终端单独写代码。`terminal` 模块负责询问终端是否支持 kitty 协议，并且是唯一往终端写字节的地方。不支持就继续显示占位符。图片的位置用 kitty 协议的 Unicode 占位符表示：每个图片格子是一个普通字符，像文字一样画进 ratatui 的缓冲区，所以滚动、裁剪、重画和 tmux 下的定位都跟着文字走。图片数据按需从 transcript 记录里读，原文件删了也能显示。

## 目录结构

```
src/
├── event.rs                   Section::Image 取代图片的 Placeholder
├── terminal.rs                由 terminal_background.rs 改名：终端查询、kitty 支持探测、写图片命令
├── providers/tool.rs          mcp_content 和 read_output 产出 Section::Image
└── ui/
    ├── inline_images/
    │   ├── mod.rs             InlineImages：上传、虚拟摆放、占位符格子
    │   └── raster.rs          读记录、解码、按显示尺寸缩放、编码 PNG，带有上限的缓存
    ├── tool_detail.rs         Body::Image：按缩放后的高度占行
    ├── event_list.rs          缩放后作废详情布局缓存
    └── input.rs               缩放和复原的鼠标操作
```

`terminal.rs` 现在只查背景色，改名后管所有“问终端、写终端”的事。`inline_images` 只生成要发给终端的命令和要画进缓冲区的占位符，自己不碰终端。`src/images.rs` 已经被输入框的 PNG 附件占用，`event::Image` 也已经表示“附在提示词里的图片”，所以新模块叫 `inline_images`，数据类型叫 `ImageBlock`，表示 transcript 记录里的一个图片内容块。

## 关键类型与接口

```rust
// event.rs
pub enum Section {
    /// An image block; drawn inline when the terminal supports images, else as its placeholder text.
    Image(ImageBlock),
    ...
}

/// An image content block inside a transcript record, read on demand.
pub struct ImageBlock {
    pub media_type: String,
    pub record: SourceRecord,
    /// The JSON pointer of the block's base64 data within the record.
    pub pointer: String,
}

// terminal.rs
/// What the terminal reported when the dashboard entered it.
pub struct Report {
    /// The default background colour, when the terminal reports one.
    pub background: Option<(u8, u8, u8)>,
    /// Whether the terminal accepted a kitty graphics query.
    pub supports_images: bool,
    /// One cell's size in pixels, when the terminal reports its pixel size.
    pub cell_pixels: Option<(u16, u16)>,
}

/// Queries the terminal on `/dev/tty`, through tmux when inside it; the terminal must be in raw mode.
pub fn query(timeout: Duration) -> io::Result<Report>;

/// Sends kitty graphics commands to the terminal, wrapped for tmux passthrough when inside tmux.
pub fn send_image_commands(out: &mut impl Write, commands: &[u8]) -> io::Result<()>;

// ui/inline_images/mod.rs
/// Images uploaded to the terminal and the commands that keep them current.
/// Exists only while the terminal supports images.
pub struct InlineImages { ... }

impl InlineImages {
    /// Images for a terminal whose cells are `cell_pixels` in size, or 1:2 when unknown.
    pub fn new(cell_pixels: Option<(u16, u16)>) -> Self;
    /// The cells `image` covers in a panel `width` cells wide at `zoom`;
    /// `None` when its data cannot be read or decoded.
    pub fn size(&mut self, image: &ImageBlock, width: u16, zoom: Zoom) -> Option<(u16, u16)>;
    /// Draws the part of `image` that falls in `area`, starting at image row `first_row`,
    /// with `zoom` choosing the visible columns. Queues the upload and placement the first
    /// time `image` is drawn at this size.
    pub fn draw(&mut self, image: &ImageBlock, zoom: Zoom, first_row: u16, area: Rect, buf: &mut Buffer);
    /// Graphics commands queued since the last call.
    pub fn take_commands(&mut self) -> Vec<u8>;
}

/// An image's zoom; the default fits the panel width.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Zoom {
    /// Steps of 1.25x beyond fitting the panel width.
    pub level: u8,
    /// The fraction of the zoomed image's width left of the panel.
    pub left: f32,
}
```

`terminal::Report` 是终端报告的结果：背景色用来选浅色或深色主题，`supports_images` 表示能不能画图，`cell_pixels` 用来算图片占几行。是否在 tmux 里不是终端报告的，而是看本进程的 `$TMUX`，所以不放在 `Report` 里，由 `terminal.rs` 内部判断。程序持有 `Option<InlineImages>`：`supports_images` 为真时才是 `Some`，`None` 就画占位符文字。

`InlineImages` 把图片 id、占位符字符和颜色编码都藏在内部，`tool_detail` 只用 `size` 排版、用 `draw` 画图。每张图只上传一次，占一个图片 id，id 用 256 色的前景色编码在占位符格子上，所以同一时间最多显示 255 张不同的图。缩放改变显示尺寸时，只发一条命令替换这张图的虚拟摆放，不重新上传，也不占新的 id。`draw` 只写落在 `area` 里的那些格子：滚出屏幕或者放大后超出面板宽度的部分不写，终端就不画，裁剪不需要另外处理。

## 交互

| 操作 | 效果 |
|---|---|
| 无修饰键左键单击图片 | 进入这张图的缩放模式；不触发文字选择，也不计入双击复制 |
| 缩放模式下，指针在这张图上向上滚 | 以指针所在位置为中心放大一级 |
| 缩放模式下，指针在这张图上向下滚 | 以指针所在位置为中心缩小一级，最小回到适配面板宽度 |
| 再单击这张图或按 Esc | 退出缩放模式，并复原到适配面板宽度；Esc 不再交给原来的处理 |
| 切换事件或焦点离开详情面板 | 退出缩放模式，并复原到适配面板宽度 |
| 未进入缩放模式，或指针不在当前缩放图片上时滚轮 | 照常滚动详情；图片按行参与展开和滚动，可以只露出一部分 |
| 方向键、翻页键 | 照常滚动详情 |
| 图片外单击 | 保持原有行为 |

系统可能占用修饰键加滚轮，Option、Shift 在 iTerm→tmux 里不上报，所以用点击进入缩放模式。默认大小是适配详情面板宽度，但不会把小图放大到超过原图尺寸。缩放倍数每帧最多变一级（1.25 倍），同一帧里连续到达的滚轮事件合并成一级。最大放到原图像素的 8 倍，而且一张图最多占 297×297 格（kitty 占位符能编码的行列上限），哪个先到算哪个。放大后横向显示哪一段由缩放中心决定。键盘不提供缩放。

## 探测

`terminal::query` 在每次进入终端时调用一次，窗口大小改变时重新读格子像素大小。kitty 图形查询并入现有的背景色查询，同样用 DA1 应答结束等待：

- **kitty 支持**：发 `ESC _G i=31,s=1,v=1,a=q,t=d,f=24 ; AAAA ESC \`，收到 `ESC _G i=31;OK ESC \` 就置 `supports_images`。在 iTerm2 → tmux 里实测收到了 `OK`。
- **tmux**：`$TMUX` 非空时，查询和所有图片命令都包成 `ESC P tmux; … ESC \`，内容里的 ESC 加倍。这要求 tmux 开了 `allow-passthrough`。包过的 DA1 由外层终端回答，所以经 tmux 也能收到 kitty 的应答。
- **格子像素大小**：用 `TIOCGWINSZ` 读 `ws_xpixel`/`ws_ypixel`，除以行列数，填进 `cell_pixels`，读到 0 就是 `None`。它只用来按原图宽高比算出图片占几行，终端自己按格子大小缩放图片。

## 集成

```
┌────────────┐  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  ┌──────────────┐
│ provider   │─▶│ Section::   │─▶│ tool_detail  │─▶│ 缓冲区写占位符│─▶│ draw 之前    │
│ 图片块位置 │  │ Image       │  │ Body::Image  │  │ 排上传命令    │  │ terminal 写出│
└────────────┘  └─────────────┘  └──────────────┘  └───────────────┘  └──────────────┘
```

- **数据**：`tool.rs` 的 `mcp_content`（Claude 和 Codex 的 MCP 结果，包括截图）和 `read_output`（Read 读图片），把 `Section::Placeholder("[image …]")` 换成 `Section::Image`，只记图片块所在的记录和 JSON 指针，不把 base64 留在内存里。
- **布局**：`tool_detail.rs` 的 `Body` 加一个 `Image` 变体。没有 `InlineImages`，或者 `size` 返回 `None` 时，它画成原来的占位符（一行暗色文字）。否则它按 `size` 返回的高度占若干行，这些行和其他正文一样参与 `SectionScroll` 的行预算和展开：只分到一部分行时，只画这些行，最后一行不画 `…`。复制、选择和搜索时，只有图片的第一行带占位符文字，其余行是空的。
- **缓存作废**：缩放或复原后，把 `viewport.event_detail_layout` 置空，让下一帧按新高度重新排版。
- **绘制**：渲染时，`Body::Image` 调 `InlineImages::draw` 把占位符写进缓冲区。真正的终端循环在 `terminal.draw` 之前，用 `terminal::send_image_commands` 把 `take_commands` 排队的上传和摆放命令写出去，这样占位符字符到达终端时图片已经在了。`draw_at` 对任意后端通用，测试后端只断言缓冲区里的占位符。
- **输入**：在 `input.rs` 的鼠标分发里，图片上的无修饰键左键单击切换缩放模式，排在 `hint_mouse`、`handle_panel_mouse` 和文字选择之前，也不计入双击复制。只有缩放模式下指针所在的当前图片响应无修饰键 `ScrollUp`/`ScrollDown`；其他滚轮事件照常滚动详情。缩放模式下的 Esc 只退出模式并复原，方向键和翻页键照常滚动。
- **状态**：每张图的 `Zoom` 存在 `EventDetailScroll` 里，按事件和图片序号区分；缩放模式记录当前图片。退出模式时将当前图片换回 `Zoom::default()`。切换事件或焦点离开详情面板时退出并复原，不恢复先前的缩放模式。`SectionScroll` 是事件详情和任务详情共用的，不放在那里。
- **释放**：`InlineImages` 记着每张已上传的图，id 用满 255 个或者总字节超过上限时，按最久没用的顺序发 `a=d,d=I` 删掉，下次显示时再上传。

## 测试

Read 读图片和 MCP 截图的结果目前都还没有真实样本。写测试之前，先在 Preview 里打开一页幻灯片截图，用这张图走一次真实的 Read 和 MCP 截图，再把这两条 transcript 记录放进 `tests/fixtures/`。样本里不能带本机信息：路径、用户名和窗口里其他内容都要替换或裁掉。

## 性能

每张图在每个显示尺寸下只编码、上传一次，之后滚动、裁剪、重画都只是写占位符字符，开销和画同样多的文字一样。缩放时每一级会发一条摆放命令，如果终端那边按格子缩放效果不好，再在本地按新尺寸重新编码并上传。

## 未核实与风险

- tmux 切窗口、放大面板之后，图片会不会跟着占位符自动重画，还没实测。
- 以上只在 iTerm2 3.7.3 → tmux next-3.9 里测过，别的终端没测。
