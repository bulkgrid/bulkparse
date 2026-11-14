"""Conservative ruled-grid recovery from pixels and OCR boxes; no document IDs."""


def bounds(line):
    return (min(p[0] for p in line['quad']), min(p[1] for p in line['quad']),
            max(p[0] for p in line['quad']), max(p[1] for p in line['quad']))


def centers(values):
    groups = []
    for value in values:
        if groups and value == groups[-1][-1] + 1:
            groups[-1].append(value)
        else:
            groups.append([value])
    return [round(sum(group) / len(group)) for group in groups]


def grid(horizontal, vertical, box, lines):
    import numpy as np
    x, y, width, height = box
    xs = centers(np.flatnonzero(np.count_nonzero(vertical[y:y+height, x:x+width], axis=0) > height * .25))
    ys = centers(np.flatnonzero(np.count_nonzero(horizontal[y:y+height, x:x+width], axis=1) > width * .25))
    xs, ys = [v+x for v in xs], [v+y for v in ys]
    columns, rows = len(xs)-1, len(ys)-1
    if not (2 <= columns <= 64 and 2 <= rows <= 128 and columns*rows <= 4096):
        return None
    if min(b-a for a, b in zip(xs, xs[1:])) < 12 or min(b-a for a, b in zip(ys, ys[1:])) < 12:
        return None

    def wall(axis, at, start, end):
        mask = vertical if axis == 0 else horizontal
        section = mask[start+3:end-2, max(0, at-2):at+3] if axis == 0 else mask[max(0, at-2):at+3, start+3:end-2]
        return float(np.mean(np.any(section, axis=1 if axis == 0 else 0)))

    for r in range(rows):
        if min(wall(0, xs[0], ys[r], ys[r+1]), wall(0, xs[-1], ys[r], ys[r+1])) < .85:
            return None
    for c in range(columns):
        if min(wall(1, ys[0], xs[c], xs[c+1]), wall(1, ys[-1], xs[c], xs[c+1])) < .85:
            return None
    parents = list(range(rows*columns))

    def root(n):
        while parents[n] != n:
            parents[n] = parents[parents[n]]
            n = parents[n]
        return n

    for r in range(rows):
        for c in range(columns):
            for other, strength in ((r*columns+c+1, wall(0, xs[c+1], ys[r], ys[r+1])) if c+1 < columns else (None, 1),
                                    ((r+1)*columns+c, wall(1, ys[r+1], xs[c], xs[c+1])) if r+1 < rows else (None, 1)):
                if .2 < strength < .85:
                    return None
                if other is not None and strength <= .2:
                    parents[root(other)] = root(r*columns+c)
    groups = {}
    for r in range(rows):
        for c in range(columns):
            groups.setdefault(root(r*columns+c), []).append((r, c))
    cells = []
    for positions in groups.values():
        top, left = min(r for r, _ in positions), min(c for _, c in positions)
        bottom, right = max(r for r, _ in positions)+1, max(c for _, c in positions)+1
        if len(positions) != (bottom-top)*(right-left):
            return None
        cells.append((top, left, bottom, right, []))
    claimed = set()
    for index, line in enumerate(lines):
        l, t, r, b = bounds(line)
        if r <= xs[0] or l >= xs[-1] or b <= ys[0] or t >= ys[-1]:
            continue
        matches = [cell for cell in cells if l >= xs[cell[1]]-2 and r <= xs[cell[3]]+2
                   and t >= ys[cell[0]]-2 and b <= ys[cell[2]]+2]
        if len(matches) != 1:
            return None
        matches[0][4].append((t, l, line['text']))
        claimed.add(index)
    if len(claimed) < 2:
        return None
    result = [[] for _ in range(rows)]
    for top, left, bottom, right, text in sorted(cells):
        result[top].append({'text': '\n'.join(v[2] for v in sorted(text)), 'row_span': bottom-top, 'col_span': right-left})
    return {'bbox': [float(xs[0]), float(ys[0]), float(xs[-1]), float(ys[-1])],
            'label': 'ruled_table', 'content': {'type': 'table', 'rows': result, 'header_rows': 0}}, claimed


def regions_from_grid(pixels, lines):
    import cv2
    import numpy as np
    height, width = pixels.shape[:2]
    gray = cv2.cvtColor(pixels, cv2.COLOR_BGR2GRAY)
    _, binary = cv2.threshold(gray, 0, 255, cv2.THRESH_BINARY_INV | cv2.THRESH_OTSU)
    horizontal = cv2.morphologyEx(binary, cv2.MORPH_OPEN, np.ones((1, max(24, width//40)), np.uint8))
    vertical = cv2.morphologyEx(binary, cv2.MORPH_OPEN, np.ones((max(24, height//40), 1), np.uint8))
    contours, _ = cv2.findContours(cv2.bitwise_or(horizontal, vertical), cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    boxes = sorted((cv2.boundingRect(contour) for contour in contours), key=lambda b: b[2]*b[3], reverse=True)
    regions, used = [], set()
    for box in boxes[:128]:
        if box[2] < 80 or box[3] < 60:
            continue
        found = grid(horizontal, vertical, box, lines)
        if found is None or found[1] & used:
            continue
        region, claimed = found
        used.update(claimed)
        regions.append(region)
    if not regions:
        return None
    # Side-by-side prose/table layouts need a layout model. Keep the existing
    # line-order path rather than flattening interleaved columns into regions.
    for index, line in enumerate(lines):
        if index not in used:
            _, top, _, bottom = bounds(line)
            if any(top < region["bbox"][3] and bottom > region["bbox"][1] for region in regions):
                return None
    for index, line in enumerate(lines):
        if index not in used:
            regions.append({'bbox': list(bounds(line)), 'label': 'text', 'confidence': line['confidence'],
                            'content': {'type': 'text', 'text': line['text']}})
    if len(regions) > 1024:
        return None
    regions.sort(key=lambda region: (region['bbox'][1], region['bbox'][0]))
    for order, region in enumerate(regions):
        region['order'] = order
    return regions
