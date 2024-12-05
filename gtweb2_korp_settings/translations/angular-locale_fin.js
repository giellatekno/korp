'use strict';
angular.module("ngLocale", [], ["$provide", function($provide) {
var PLURAL_CATEGORY = {ZERO: "zero", ONE: "one", TWO: "two", FEW: "few", MANY: "many", OTHER: "other"};
function getDecimals(n) {
  n = n + '';
  var i = n.indexOf('.');
  return (i == -1) ? 0 : n.length - i - 1;
}

function getVF(n, opt_precision) {
  var v = opt_precision;

  if (undefined === v) {
    v = Math.min(getDecimals(n), 3);
  }

  var base = Math.pow(10, v);
  var f = ((n * base) | 0) % base;
  return {v: v, f: f};
}

$provide.value("$locale", {
  "DATETIME_FORMATS": {
    "AMPMS": [
      "FIN AM",
      "FIN PM"
    ],
    "DAY": [
      "FIN søndag",
      "FIN mandag",
      "FIN tirsdag",
      "FIN onsdag",
      "FIN torsdag",
      "FIN fredag",
      "FIN lørdag"
    ],
    "ERANAMES": [
      "FIN før Kristus",
      "FIN etter Kristus"
    ],
    "ERAS": [
      "FIN f.Kr.",
      "FIN e.Kr."
    ],
    "FIRSTDAYOFWEEK": 1,
    "MONTH": [
      "FIN januar",
      "FIN februar",
      "FIN mars",
      "FIN april",
      "FIN mai",
      "FIN juni",
      "FIN juli",
      "FIN august",
      "FIN september",
      "FIN oktober",
      "FIN november",
      "FIN desember"
    ],
    "SHORTDAY": [
      "FIN søn",
      "FIN man",
      "FIN tir",
      "FIN ons",
      "FIN tor",
      "FIN fre",
      "FIN lør"
    ],
    "SHORTMONTH": [
      "FIN jan.",
      "FIN feb.",
      "FIN mars",
      "FIN apr.",
      "FIN maj",
      "FIN juni",
      "FIN juli",
      "FIN aug.",
      "FIN sep.",
      "FIN okt.",
      "FIN nov.",
      "FIN dec."
    ],
    "STANDALONEMONTH": [
      "FIN Januar",
      "FIN Februar",
      "FIN Mars",
      "FIN April",
      "FIN Mai",
      "FIN Juni",
      "FIN Juli",
      "FIN August",
      "FIN September",
      "FIN Oktober",
      "FIN November",
      "FIN Desember"
    ],
    "WEEKENDRANGE": [
      5,
      6
    ],
    "fullDate": "EEEE d MMMM y",
    "longDate": "d MMMM y",
    "medium": "d MMM y HH:mm:ss",
    "mediumDate": "d MMM y",
    "mediumTime": "HH:mm:ss",
    "short": "y-MM-dd HH:mm",
    "shortDate": "y-MM-dd",
    "shortTime": "HH:mm"
  },
  "NUMBER_FORMATS": {
    "CURRENCY_SYM": "kr",
    "DECIMAL_SEP": ",",
    "GROUP_SEP": "\u00a0",
    "PATTERNS": [
      {
        "gSize": 3,
        "lgSize": 3,
        "maxFrac": 3,
        "minFrac": 0,
        "minInt": 1,
        "negPre": "-",
        "negSuf": "",
        "posPre": "",
        "posSuf": ""
      },
      {
        "gSize": 3,
        "lgSize": 3,
        "maxFrac": 2,
        "minFrac": 2,
        "minInt": 1,
        "negPre": "-",
        "negSuf": "\u00a0\u00a4",
        "posPre": "",
        "posSuf": "\u00a0\u00a4"
      }
    ]
  },
  "id": "no",
  "localeID": "no",
  "pluralCat": function(n, opt_precision) {  var i = n | 0;  var vf = getVF(n, opt_precision);  if (i == 1 && vf.v == 0) {    return PLURAL_CATEGORY.ONE;  }  return PLURAL_CATEGORY.OTHER;}
});
}]);
