local resound = require("resound")
resound.addSample({name = "drums", path = "drums.wav", volume = 1})
resound.setParamValue("drums", "param", 0.5)
