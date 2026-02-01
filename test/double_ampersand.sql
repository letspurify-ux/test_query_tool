PROMPT === [DOUBLE AMPERSAND] Start ===
PROMPT Expect one prompt for &&v_name, then reused.
SELECT '&&v_name' AS first_use FROM dual;
SELECT '&&v_name' AS second_use FROM dual;
PROMPT === [DOUBLE AMPERSAND] End ===
