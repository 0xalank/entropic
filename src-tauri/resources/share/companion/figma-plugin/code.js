const UI_HTML = __html__;

figma.showUI(UI_HTML, { visible: false, width: 320, height: 120 });

function serializeNode(node) {
  return {
    id: node.id,
    name: node.name,
    type: node.type,
    visible: "visible" in node ? node.visible : true,
    locked: "locked" in node ? node.locked : false,
    x: "x" in node ? node.x : null,
    y: "y" in node ? node.y : null,
    width: "width" in node ? node.width : null,
    height: "height" in node ? node.height : null,
    layoutMode: "layoutMode" in node ? node.layoutMode : null,
    componentPropertyReferences:
      "componentPropertyReferences" in node ? node.componentPropertyReferences : null
  };
}

function selectionPayload() {
  const selection = figma.currentPage.selection.map(serializeNode);
  return {
    type: "selection",
    source: "figma-plugin",
    fileKey: figma.fileKey,
    fileName: figma.root.name,
    page: {
      id: figma.currentPage.id,
      name: figma.currentPage.name
    },
    selection,
    selectionCount: selection.length,
    updatedAt: Date.now()
  };
}

function sendSelection() {
  figma.ui.postMessage(selectionPayload());
}

figma.on("selectionchange", sendSelection);
figma.on("currentpagechange", sendSelection);

figma.ui.onmessage = (message) => {
  if (!message || typeof message !== "object") return;
  if (message.type === "request-selection") {
    sendSelection();
    return;
  }
  if (message.type === "create_frame") {
    const frame = figma.createFrame();
    frame.name = typeof message.name === "string" && message.name.trim()
      ? message.name.trim()
      : "Entropic Frame";
    frame.x = Number.isFinite(message.x) ? message.x : 0;
    frame.y = Number.isFinite(message.y) ? message.y : 0;
    frame.resize(
      Number.isFinite(message.width) && message.width > 0 ? message.width : 720,
      Number.isFinite(message.height) && message.height > 0 ? message.height : 480
    );
    figma.currentPage.appendChild(frame);
    figma.currentPage.selection = [frame];
    figma.viewport.scrollAndZoomIntoView([frame]);
    figma.ui.postMessage({
      type: "command_result",
      commandId: message.id || null,
      commandType: "create_frame",
      ok: true,
      node: serializeNode(frame),
      updatedAt: Date.now()
    });
    sendSelection();
  }
};

sendSelection();
