const core = require('./quant_core.js');

function evaluate_risk(x, y, factor) {
    const sum = core.add_values(x, y);
    return sum * factor;
}

module.exports = { evaluate_risk };
