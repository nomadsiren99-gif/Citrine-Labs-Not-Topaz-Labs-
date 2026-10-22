// Citrine Labs: turns the active layer into a Smart Object, so the Citrine filter that runs next
// is kept as a Smart Filter (non-destructive). Run it from File > Scripts, then choose Filter > Citrine.
// Untested in Photoshop 2026: check that the layer becomes a Smart Object before applying the filter.
#target photoshop

(function () {
    if (app.documents.length === 0) {
        alert("Citrine Labs: open a document first.");
        return;
    }
    var layer = app.activeDocument.activeLayer;
    if (layer.kind === LayerKind.SMARTOBJECT) {
        return; // already a Smart Object: the filter will be a Smart Filter
    }
    // Layer > Smart Objects > Convert to Smart Object
    executeAction(stringIDToTypeID("newPlacedLayer"), new ActionDescriptor(), DialogModes.NO);
})();
