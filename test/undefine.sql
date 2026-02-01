PROMPT === [UNDEFINE] Start ===
SET DEFINE ON
PROMPT Defining v_name as Alice.
DEFINE v_name Alice
SELECT '&v_name' AS first_use FROM dual;
PROMPT Undefining v_name - next &&v_name should prompt again.
UNDEFINE v_name
SELECT '&&v_name' AS second_use FROM dual;
UNDEFINE v_name
SELECT '&&v_name' AS third_use FROM dual;
UNDEFINE v_name
PROMPT === [UNDEFINE] End ===
